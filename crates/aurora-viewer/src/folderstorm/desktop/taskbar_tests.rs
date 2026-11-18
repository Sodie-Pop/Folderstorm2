//! Frame tests of the taskbar: real `ui::widgets::Floater` windows, drawn
//! as the app draws them (most before the taskbar, one after it), the
//! hook of `Floater::show` telling the taskbar what they are. They fail if
//! that hook is lost at an upstream merge.

use super::store::{Desktop, Taskbar};
use super::taskbar::{self, Env};
use super::{demo, place};
use crate::folderstorm::viewport::{self, MonitorLayout};
use crate::ui::widgets::{self, CascadeGroup, Floater};
use egui::{Id, Pos2, Rect, pos2, vec2};

/// A window of the test.
#[derive(Clone)]
struct Win {
    id: &'static str,
    title: &'static str,
    open: bool,
    cascade: bool,
    /// It keeps a compact body when minimized (as Performance).
    compact: bool,
    attention: bool,
    /// Drawn after the taskbar in the frame (as some of the app's are).
    late: bool,
}

fn win(id: &'static str, title: &'static str) -> Win {
    Win {
        id,
        title,
        open: true,
        cascade: false,
        compact: false,
        attention: false,
        late: false,
    }
}

/// A viewer window of 1920 × 400 over three monitors, the world on the
/// middle one, the taskbar on.
struct Bench {
    ctx: egui::Context,
    p: crate::theme::Palette,
    icons: crate::ui::icons::Icons,
    world: Option<[u32; 4]>,
    desktop: Desktop,
    hidden: bool,
    mouselook: bool,
    focus: bool,
    wins: Vec<Win>,
    /// The texts of the last frame, on the screen.
    texts: Vec<(String, Rect)>,
    /// Every frame's texts since [`Bench::watch`].
    watched: Option<Vec<Vec<(String, Rect)>>>,
    pointer: Pos2,
    time: f64,
}

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, pos2(1920.0, 400.0));

impl Bench {
    fn new(wins: Vec<Win>) -> Self {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let mut bench = Bench {
            ctx,
            p,
            icons,
            world: Some([640, 0, 640, 400]),
            desktop: Desktop {
                taskbar: Taskbar {
                    on: true,
                    slot: Some(2),
                    all_windows: false,
                },
                ..Default::default()
            },
            hidden: false,
            mouselook: false,
            focus: false,
            wins,
            texts: Vec::new(),
            watched: None,
            pointer: pos2(960.0, 200.0),
            time: 0.0,
        };
        bench.frames(3);
        bench
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        viewport::set_world_area(&self.ctx, self.world);
        viewport::set_monitors(
            &self.ctx,
            MonitorLayout {
                window: [0, 0, 1920, 400],
                monitors: vec![[0, 0, 640, 400], [640, 0, 640, 400], [1280, 0, 640, 400]],
            },
        );
        let world = self
            .world
            .map(|[x, y, w, h]| Rect::from_min_size(pos2(x as f32, y as f32), vec2(w as f32, h as f32)));
        let p = self.p;
        let focus = std::mem::take(&mut self.focus);
        let (desktop, icons, wins) = (&self.desktop, &self.icons, &mut self.wins);
        let (hidden, mouselook) = (self.hidden, self.mouselook);
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                time: Some(self.time),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                crate::ui::close_key::begin(&ctx);
                viewport::set_bars_area(&ctx, world);
                viewport::bars::set_sides(&ctx, None);
                crate::ui::layout::set_work_area(&ctx, world.unwrap_or(SCREEN));
                let draw = |ctx: &egui::Context, n: usize, w: &mut Win| {
                    let mut floater = Floater::new(
                        w.id,
                        w.title,
                        pos2(700.0 + 20.0 * n as f32, 40.0 + 10.0 * n as f32),
                        vec2(260.0, 200.0),
                    )
                    .silent()
                    .attention(w.attention);
                    if w.cascade {
                        floater = floater.cascade(CascadeGroup::Notecard);
                    }
                    if w.compact {
                        floater = floater.compact_when_minimized(200.0);
                    }
                    let body = format!("body of {}", w.id);
                    floater.show(ctx, &p, &mut w.open, |ui| {
                        ui.label(body);
                    });
                };
                for (n, w) in wins.iter_mut().enumerate().filter(|(_, w)| !w.late) {
                    draw(&ctx, n, w);
                }
                let env = Env {
                    p: &p,
                    icons,
                    hidden,
                    mouselook,
                    focus,
                };
                taskbar::show(&ctx, desktop, &env);
                for (n, w) in wins.iter_mut().enumerate().filter(|(_, w)| w.late) {
                    draw(&ctx, n, w);
                }
            },
        );
        output.textures_delta.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                _ => {}
            }
        }
        self.texts.clear();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut self.texts);
        }
        // (what was taken off the screen is not seen)
        self.texts.retain(|(_, rect)| rect.intersects(SCREEN));
        if let Some(watched) = &mut self.watched {
            watched.push(self.texts.clone());
        }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![egui::Event::PointerMoved(self.pointer)]);
        }
    }

    /// From now on every frame's texts are kept.
    fn watch(&mut self) {
        self.watched = Some(Vec::new());
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
        self.frames(2);
    }

    fn click(&mut self, at: Pos2) {
        self.click_with(at, egui::PointerButton::Primary);
    }

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
        self.frames(2);
    }

    /// An entry of the menu opened by a right click at `at`.
    fn menu(&mut self, at: Pos2, label: &str) {
        self.click_with(at, egui::PointerButton::Secondary);
        let entry = self
            .text(label)
            .unwrap_or_else(|| panic!("{label:?} is in the menu: {:?}", self.texts));
        self.click(entry.center());
    }

    fn text(&self, text: &str) -> Option<Rect> {
        self.texts.iter().find(|(t, _)| t == text).map(|(_, rect)| *rect)
    }

    fn island(&self) -> Option<Rect> {
        taskbar::drawn(&self.ctx).island
    }

    /// The button of the window `id` on the taskbar.
    fn button(&self, id: &str) -> Option<Rect> {
        taskbar::drawn(&self.ctx)
            .buttons
            .iter()
            .find(|(window, _, _)| window == id)
            .map(|(_, rect, _)| *rect)
    }

    fn buttons(&self) -> Vec<String> {
        taskbar::drawn(&self.ctx).buttons.into_iter().map(|(id, _, _)| id).collect()
    }

    /// The title of the window `id` is drawn out of the taskbar: its title
    /// bar (collapsed or not) is on screen.
    fn title_bar(&self, id: &str) -> bool {
        let title = self.wins.iter().find(|w| w.id == id).expect("a window of the test").title;
        let island = self.island();
        self.texts
            .iter()
            .any(|(text, rect)| text == title && !island.is_some_and(|island| island.contains_rect(*rect)))
    }

    /// The body of the window `id` is drawn: it is expanded.
    fn body(&self, id: &str) -> bool {
        self.text(&format!("body of {id}")).is_some()
    }

    fn rect(&self, id: &str) -> Rect {
        self.ctx.memory(|m| m.area_rect(Id::unique(id))).expect("the window was drawn")
    }

    fn collapse(&mut self, id: &str) {
        widgets::minimize_floater(&self.ctx, id);
        self.frames(3);
    }

    fn open(&self, id: &str) -> bool {
        self.wins.iter().find(|w| w.id == id).expect("a window of the test").open
    }

    /// The windows of the test from the back to the front.
    fn order(&self) -> Vec<&'static str> {
        self.ctx.memory(|m| {
            m.layer_ids()
                .filter_map(|layer| self.wins.iter().find(|w| Id::unique(w.id) == layer.id).map(|w| w.id))
                .collect()
        })
    }
}

#[test]
fn the_icons_of_the_taskbar_are_embedded() {
    for icon in taskbar::ICONS {
        assert!(crate::ui::icons::embedded(icon), "{icon} is not an embedded Tabler icon");
    }
}

/// The hook of `Floater::show`: a collapsed window is not drawn while the
/// taskbar holds it, its button stands for it, and a click restores it
/// where it was, at the size it had.
#[test]
fn a_collapsed_window_is_held_as_a_button_and_restored_where_it_was() {
    let mut b = Bench::new(vec![win("alpha", "Alpha window"), win("beta", "Beta window")]);
    let island = b.island().expect("the taskbar stands");
    // along the bottom of the third monitor's room
    assert_eq!(
        island,
        Rect::from_min_max(pos2(1288.0, 392.0 - place::TASKBAR_H), pos2(1912.0, 392.0))
    );
    assert!(b.buttons().is_empty(), "open windows are not listed");
    assert!(b.text("No collapsed windows").is_some());
    assert!(b.body("alpha") && b.title_bar("alpha"));
    let before = b.rect("alpha");

    b.collapse("alpha");
    assert!(widgets::floater_minimized(&b.ctx, "alpha"), "collapsed, as Aurora keeps it");
    assert!(taskbar::held(&b.ctx, "alpha"));
    assert!(!b.title_bar("alpha"), "no title bar: {:?}", b.texts);
    assert!(!b.body("alpha"));
    assert_eq!(b.buttons(), ["alpha"]);
    let button = b.button("alpha").expect("its button");
    assert!(island.contains_rect(button));
    assert!(
        b.text("Alpha window").is_some_and(|text| button.contains_rect(text)),
        "its title on its button"
    );
    // the other window is as it was
    assert!(b.body("beta") && b.title_bar("beta"));

    // a click restores it, and no frame shows its collapsed title bar
    b.watch();
    b.click(button.center());
    assert!(!widgets::floater_minimized(&b.ctx, "alpha"));
    assert!(b.body("alpha") && b.title_bar("alpha"));
    assert!(b.buttons().is_empty());
    assert_eq!(b.rect("alpha").min, before.min, "where it was");
    assert_eq!(b.rect("alpha").size(), before.size(), "at the size it had");
    assert_eq!(b.order().last(), Some(&"alpha"), "in front");
    let island = b.island().expect("the taskbar");
    for (n, texts) in b.watched.take().expect("watched").iter().enumerate() {
        let bar = texts.iter().any(|(t, rect)| t == "Alpha window" && !island.contains_rect(*rect));
        let body = texts.iter().any(|(t, _)| t == "body of alpha");
        assert!(!bar || body, "frame {n}: a collapsed title bar on its way back");
    }
}

#[test]
fn off_or_without_a_margin_collapsed_windows_are_title_bars_again() {
    let mut b = Bench::new(vec![win("alpha", "Alpha window")]);
    b.collapse("alpha");
    assert!(!b.title_bar("alpha"));
    // off: Aurora's title bar, where the window stood
    b.desktop.taskbar.on = false;
    b.frames(3);
    assert!(b.island().is_none());
    assert!(b.title_bar("alpha") && !b.body("alpha"));
    assert!(!taskbar::held(&b.ctx, "alpha"));
    b.desktop.taskbar.on = true;
    b.frames(3);
    assert!(!b.title_bar("alpha"));
    // the world fills the window: no margin, no taskbar
    b.world = None;
    b.frames(3);
    assert!(b.island().is_none());
    assert!(b.title_bar("alpha"));
    b.world = Some([640, 0, 640, 400]);
    b.frames(3);
    assert!(!b.title_bar("alpha") && b.button("alpha").is_some());
    // the monitor chosen for it has no room (the world is on it)
    b.desktop.taskbar.slot = Some(1);
    b.frames(3);
    assert!(b.island().is_none());
    assert!(b.title_bar("alpha"));
    // automatic: the widest room, the first of two as wide
    b.desktop.taskbar.slot = None;
    b.frames(3);
    assert_eq!(b.island().map(|island| island.left()), Some(8.0));
    assert!(!b.title_bar("alpha"));
}

/// Risk 2 of the plan: a cascade group forgets a window it has not heard
/// of for two frames, and places it again as a new one. A held window
/// tells its group it is hidden, from the hook.
#[test]
fn a_cascade_window_keeps_its_place_after_being_held() {
    let mut first = win("notecard-a", "First notecard");
    first.cascade = true;
    let mut second = win("notecard-b", "Second notecard");
    second.cascade = true;
    let mut b = Bench::new(vec![first, second]);
    b.frames(3);
    // moved away from where its group puts a new window
    widgets::move_floater(&b.ctx, "notecard-b", pos2(900.0, 150.0));
    b.frames(3);
    let before = b.rect("notecard-b");
    assert_eq!(before.min, pos2(900.0, 150.0));
    b.collapse("notecard-b");
    b.frames(10);
    assert!(taskbar::held(&b.ctx, "notecard-b"));
    assert!(!b.title_bar("notecard-b"));
    b.click(b.button("notecard-b").expect("its button").center());
    b.frames(3);
    assert!(b.body("notecard-b"));
    assert_eq!(b.rect("notecard-b").min, before.min, "not placed again as a new notecard");
    assert_eq!(b.rect("notecard-b").size(), before.size());
}

#[test]
fn a_window_with_a_compact_body_is_never_held() {
    let mut performance = win("performance", "Performance");
    performance.compact = true;
    let mut b = Bench::new(vec![performance]);
    b.collapse("performance");
    assert!(widgets::floater_minimized(&b.ctx, "performance"));
    assert!(!taskbar::held(&b.ctx, "performance"));
    assert!(b.body("performance") && b.title_bar("performance"), "its compact view");
    assert!(b.buttons().is_empty());
    // listed with every open window, as an open one
    b.desktop.taskbar.all_windows = true;
    b.frames(2);
    assert_eq!(b.buttons(), ["performance"]);
}

#[test]
fn a_button_s_menu_restores_and_closes_and_a_middle_click_closes() {
    let mut b = Bench::new(vec![
        win("alpha", "Alpha window"),
        win("beta", "Beta window"),
        win("gamma", "Gamma window"),
    ]);
    for id in ["alpha", "beta", "gamma"] {
        b.collapse(id);
    }
    assert_eq!(b.buttons(), ["alpha", "beta", "gamma"], "in the order they were collapsed");
    b.menu(b.button("beta").expect("a button").center(), "Restore");
    assert!(b.body("beta"));
    assert_eq!(b.buttons(), ["alpha", "gamma"]);
    // Close: as the window's cross
    b.watch();
    b.menu(b.button("alpha").expect("a button").center(), "Close");
    assert!(!b.open("alpha"));
    assert_eq!(b.buttons(), ["gamma"]);
    assert!(!widgets::floater_minimized(&b.ctx, "alpha"), "it opens again expanded");
    let island = b.island().expect("the taskbar");
    for (n, texts) in b.watched.take().expect("watched").iter().enumerate() {
        assert!(
            !texts.iter().any(|(t, rect)| t == "Alpha window" && !island.contains_rect(*rect)),
            "frame {n}: a title bar on its way out"
        );
    }
    b.click_with(b.button("gamma").expect("a button").center(), egui::PointerButton::Middle);
    assert!(!b.open("gamma"));
    assert!(b.buttons().is_empty());
    // collapsed again, a window is the last of the list
    b.collapse("beta");
    b.wins[0].open = true;
    b.frames(2);
    b.collapse("alpha");
    assert_eq!(b.buttons(), ["beta", "alpha"]);
}

#[test]
fn all_open_windows_lists_them_and_a_click_collapses_or_brings_forward() {
    let mut b = Bench::new(vec![win("alpha", "Alpha window"), win("beta", "Beta window")]);
    b.desktop.taskbar.all_windows = true;
    b.frames(2);
    assert_eq!(b.buttons(), ["alpha", "beta"], "in the order they were opened");
    assert!(b.text("No open windows").is_none());
    // the one behind comes to the front
    widgets::bring_floater_front(&b.ctx, "beta");
    b.frames(2);
    assert_eq!(b.order().last(), Some(&"beta"));
    b.click(b.button("alpha").expect("a button").center());
    assert_eq!(b.order().last(), Some(&"alpha"));
    assert!(b.body("alpha") && !widgets::floater_minimized(&b.ctx, "alpha"));
    // the one in front is collapsed, and held like any collapsed window
    b.click(b.button("alpha").expect("a button").center());
    assert!(widgets::floater_minimized(&b.ctx, "alpha"));
    assert!(taskbar::held(&b.ctx, "alpha"));
    assert!(!b.title_bar("alpha"));
    assert_eq!(b.buttons(), ["alpha", "beta"], "it keeps its place");
    // and restored by a click
    b.click(b.button("alpha").expect("a button").center());
    assert!(b.body("alpha"));
    // the menu of an open window
    b.menu(b.button("beta").expect("a button").center(), "Collapse");
    assert!(taskbar::held(&b.ctx, "beta"));
    b.menu(b.button("beta").expect("a button").center(), "Restore");
    assert!(b.body("beta"));
    b.menu(b.button("alpha").expect("a button").center(), "Bring to front");
    assert_eq!(b.order().last(), Some(&"alpha"));
    // nothing open
    b.wins.iter_mut().for_each(|w| w.open = false);
    b.frames(3);
    assert!(b.buttons().is_empty());
    assert!(b.text("No open windows").is_some());
}

/// A collapsed window that was on top leaves the front to the next one
/// (Aurora's Ctrl+W closes the topmost window, collapsed or not).
#[test]
fn a_held_window_leaves_the_front_to_the_next_one() {
    let mut b = Bench::new(vec![win("alpha", "Alpha window"), win("beta", "Beta window")]);
    widgets::bring_floater_front(&b.ctx, "alpha");
    b.frames(2);
    assert_eq!(b.order(), ["beta", "alpha"]);
    b.collapse("alpha");
    assert_eq!(b.order(), ["alpha", "beta"]);
    // Ctrl+W closes the window that is seen
    b.frame(vec![egui::Event::Key {
        key: egui::Key::W,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }]);
    b.frames(2);
    assert!(b.open("alpha") && !b.open("beta"));
    assert!(taskbar::held(&b.ctx, "alpha"));
}

#[test]
fn too_many_buttons_narrow_to_their_icon_and_the_rest_go_in_more() {
    const IDS: [&str; 20] = [
        "w00", "w01", "w02", "w03", "w04", "w05", "w06", "w07", "w08", "w09", "w10", "w11", "w12", "w13", "w14", "w15", "w16", "w17",
        "w18", "w19",
    ];
    const TITLES: [&str; 20] = [
        "Window 00",
        "Window 01",
        "Window 02",
        "Window 03",
        "Window 04",
        "Window 05",
        "Window 06",
        "Window 07",
        "Window 08",
        "Window 09",
        "Window 10",
        "Window 11",
        "Window 12",
        "Window 13",
        "Window 14",
        "Window 15",
        "Window 16",
        "Window 17",
        "Window 18",
        "Window 19",
    ];
    let wins: Vec<Win> = IDS.iter().zip(TITLES).map(|(id, title)| win(id, title)).collect();
    let mut b = Bench::new(wins);
    for id in &IDS[..4] {
        widgets::minimize_floater(&b.ctx, id);
    }
    b.frames(3);
    let drawn = taskbar::drawn(&b.ctx);
    assert_eq!(drawn.buttons.len(), 4);
    assert!(drawn.buttons.iter().all(|(_, _, text)| *text) && drawn.more.is_none());
    for id in &IDS[4..12] {
        widgets::minimize_floater(&b.ctx, id);
    }
    b.frames(3);
    let drawn = taskbar::drawn(&b.ctx);
    assert_eq!(drawn.buttons.len(), 12);
    assert!(drawn.buttons.iter().all(|(_, _, text)| !*text), "their icon alone");
    assert!(drawn.more.is_none());
    assert!(b.text("Window 03").is_none(), "no title on the island");
    // narrower: the third monitor's room is what the bars leave of it
    b.desktop.size = super::store::Size::Small;
    for id in &IDS[12..] {
        widgets::minimize_floater(&b.ctx, id);
    }
    b.frames(3);
    let drawn = taskbar::drawn(&b.ctx);
    let island = drawn.island.expect("the taskbar");
    let more = drawn.more.expect("More");
    assert!(drawn.buttons.len() < 20 && !drawn.buttons.is_empty());
    assert!(
        drawn
            .buttons
            .iter()
            .all(|(_, rect, _)| island.contains_rect(*rect) && rect.right() <= more.left())
    );
    assert!(island.contains_rect(more));
    // the rest are in More, by their title; a click restores one
    let shown = drawn.buttons.len();
    b.click(more.center());
    assert!(b.text(TITLES[shown]).is_some(), "{:?}", b.texts);
    assert!(b.text(TITLES[19]).is_some());
    assert!(b.text(TITLES[0]).is_none());
    b.click(b.text(TITLES[19]).expect("its entry").center());
    assert!(b.body("w19"));
    assert!(!widgets::floater_minimized(&b.ctx, "w19"));
}

#[test]
fn the_taskbar_is_hidden_with_the_interface_and_the_windows_stay_held() {
    let mut late = win("late", "Late window");
    late.late = true;
    let mut b = Bench::new(vec![win("alpha", "Alpha window"), late]);
    b.collapse("alpha");
    // a window drawn after the taskbar in the frame is held as well
    b.collapse("late");
    assert_eq!(b.buttons(), ["alpha", "late"]);
    assert!(!b.title_bar("late") && taskbar::held(&b.ctx, "late"));
    b.hidden = true;
    b.frames(3);
    assert!(b.island().is_none(), "no taskbar");
    assert!(!b.title_bar("alpha") && !b.title_bar("late"), "and no title bar popping back");
    // mouselook with the viewport dropped: no margin, still held
    b.mouselook = true;
    b.world = None;
    b.frames(5);
    assert!(b.island().is_none());
    assert!(!b.title_bar("alpha") && !b.title_bar("late"));
    // out of mouselook, without a margin: title bars
    b.mouselook = false;
    b.hidden = false;
    b.frames(3);
    assert!(b.title_bar("alpha") && b.title_bar("late"));
    b.world = Some([640, 0, 640, 400]);
    b.frames(3);
    assert_eq!(b.buttons(), ["alpha", "late"]);
    b.click(b.button("late").expect("a button").center());
    assert!(b.body("late"));
}

#[test]
fn the_key_action_gives_the_taskbar_the_keyboard() {
    let mut b = Bench::new(vec![win("alpha", "Alpha window"), win("beta", "Beta window")]);
    b.collapse("alpha");
    b.collapse("beta");
    assert!(!b.ctx.memory(|m| m.has_focus(taskbar::button_id("alpha"))));
    b.focus = true;
    b.frames(2);
    assert!(b.ctx.memory(|m| m.has_focus(taskbar::button_id("alpha"))), "its first button");
    b.key(egui::Key::Enter);
    assert!(b.body("alpha"), "Enter does what a click does");
}

#[test]
fn a_window_with_something_waiting_says_so_on_its_button() {
    let mut im = win("conversations", "Conversations");
    im.attention = true;
    let mut b = Bench::new(vec![im, win("alpha", "Alpha window")]);
    b.collapse("conversations");
    b.collapse("alpha");
    let windows = taskbar::windows(&b.ctx);
    let waiting: Vec<&str> = windows.iter().filter(|w| w.attention).map(|w| w.id.as_str()).collect();
    assert_eq!(waiting, ["conversations"]);
    assert!(windows.iter().all(|w| w.held));
}

/// The taskbar's strip is out of its monitor's desktop: no shortcut under it.
#[test]
fn the_taskbar_takes_its_strip_out_of_the_desktop_s_room() {
    let b = Bench::new(Vec::new());
    let island = b.island().expect("the taskbar");
    let rooms = super::rooms(&b.ctx, &b.desktop);
    let third = rooms.iter().find(|room| room.slot == 2).expect("a room above the taskbar");
    assert_eq!(third.rect.bottom(), island.top() - place::PAD);
    let last = third.grid.rect((third.grid.cols - 1, third.grid.rows - 1));
    assert!(last.bottom() <= island.top());
    // the other monitor's desktop is whole, and so is this one without the taskbar
    let first = rooms.iter().find(|room| room.slot == 0).expect("a room");
    assert_eq!(first.rect.bottom(), 392.0);
    let mut off = b.desktop.clone();
    off.taskbar.on = false;
    let whole = super::rooms(&b.ctx, &off);
    assert_eq!(whole.iter().find(|room| room.slot == 2).expect("a room").rect.bottom(), 392.0);
    assert_eq!(super::live_slots(&b.ctx, &b.desktop), [0, 2]);
}

#[test]
fn the_demo_seeds_its_taskbar() {
    for mode in ["taskbar", "overflow"] {
        let mut desktop = Desktop::default();
        let mut viewport = crate::folderstorm::viewport::ViewportSettings::default();
        assert!(demo::seed(mode, &mut desktop, &mut viewport, 5760), "{mode}");
        assert!(desktop.taskbar.on && !desktop.taskbar.all_windows);
        assert_eq!(desktop.taskbar.slot, Some(demo::TASKBAR_SLOT));
        assert!(viewport.enabled);
    }
    let mut desktop = Desktop::default();
    let mut viewport = crate::folderstorm::viewport::ViewportSettings::default();
    assert!(demo::seed("1", &mut desktop, &mut viewport, 5760));
    assert!(!desktop.taskbar.on, "off by default");
}
