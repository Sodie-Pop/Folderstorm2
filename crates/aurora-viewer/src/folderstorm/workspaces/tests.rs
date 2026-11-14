//! Frame tests of capture and apply: an `egui::Context`, Aurora's own
//! `Floater` and `bare_window`, no graphics device.

use super::apply::{self, Live, Parts};
use super::capture;
use super::store::{Layout, Placement, Placements, Store, StoreError};
use super::windows;
use crate::folderstorm::viewport::ViewportSettings;
use crate::ui::widgets::{self, Floater};
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use std::collections::BTreeSet;

mod open;
mod switcher;
mod viewer;

const INVENTORY: &str = "inventory";
const PEOPLE: &str = "people";
const MAP: &str = "worldmap";
const MINIMAP: &str = "minimap";
/// A window that is not on the allowlist.
const PREFERENCES: &str = "preferences";

/// A viewer reduced to its windows: the store and the viewport's settings
/// as `Settings` has them, and which windows are open.
struct Viewer {
    ctx: egui::Context,
    store: Store,
    world: ViewportSettings,
    size: Vec2,
    open: BTreeSet<&'static str>,
    /// Windows the viewer cannot open now (as RLVa's @showinv has it).
    blocked: BTreeSet<&'static str>,
    time: f64,
    /// Whether `apply::frame` asked for the settings to be saved.
    saved: bool,
    /// A pretend viewer window: it runs what the layouts ask, and the
    /// interface has its size. None: no window to drive.
    window: Option<super::window::Pretend>,
    /// The monitors the app publishes with it, each (x, y, width, height).
    monitors: Vec<[i32; 4]>,
    /// `apply::frame` asked for Preferences.
    preferences: bool,
    /// World › Layouts is drawn open, at the top left.
    menu: bool,
    /// The interface is put away, or the camera is in mouselook.
    hidden: bool,
    /// What the pointer does in the next frame.
    events: Vec<egui::Event>,
    /// The texts of the last frame and where they were.
    texts: Vec<(String, Rect)>,
}

/// The viewer's windows as the app has them: open or not, opened by a
/// flag.
struct Windows<'a> {
    open: &'a mut BTreeSet<&'static str>,
    blocked: &'a BTreeSet<&'static str>,
}

impl apply::Shell for Windows<'_> {
    fn is_open(&self, window: &windows::Window) -> bool {
        self.open.contains(window.id)
    }

    fn open(&mut self, _: &egui::Context, window: &windows::Window) -> bool {
        !self.blocked.contains(window.id) && self.open.insert(window.id)
    }
}

impl Viewer {
    fn new(width: f32, height: f32) -> Self {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        Viewer {
            ctx,
            store: Store::default(),
            world: ViewportSettings::default(),
            size: vec2(width, height),
            open: BTreeSet::from([INVENTORY, PEOPLE, MAP, MINIMAP, PREFERENCES]),
            blocked: BTreeSet::new(),
            time: 0.0,
            saved: false,
            window: None,
            monitors: Vec::new(),
            preferences: false,
            menu: false,
            hidden: false,
            events: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// The same settings in a viewer just started (egui's memory is new).
    fn restarted(&self) -> Self {
        let mut next = Viewer::new(self.size.x, self.size.y);
        // through the file, as a restart reads them
        next.store = serde_json::from_str(&serde_json::to_string(&self.store).expect("saved")).expect("read");
        next.world = self.world.clone();
        next.open = self.open.clone();
        // the window where it was, as Aurora reopens it: with its border,
        // free to be resized
        next.window = self.window.as_ref().map(|window| {
            let (x, y) = window.seen.outer.unwrap_or_default();
            let mut fresh = super::window::Pretend::new(x, y, window.seen.inner.0, window.seen.inner.1);
            fresh.seen.outer = window.seen.outer;
            fresh
        });
        next.monitors = self.monitors.clone();
        next
    }

    fn live(&mut self) -> Live<'_> {
        Live {
            store: &mut self.store,
            world: &mut self.world,
        }
    }

    /// One frame as the app runs it: `apply::frame` before the pass, then
    /// the windows that are open.
    fn frame(&mut self) {
        let ctx = self.ctx.clone();
        // as the app does before the layouts' call: the desktop published,
        // the window described
        if let Some(window) = &self.window {
            self.size = vec2(window.seen.inner.0 as f32, window.seen.inner.1 as f32);
            crate::folderstorm::diagram::set_desktop(
                &ctx,
                crate::folderstorm::diagram::Desktop {
                    window: window.seen.inner,
                    position: window.seen.outer,
                    monitors: self
                        .monitors
                        .iter()
                        .map(|rect| crate::folderstorm::diagram::Monitor { rect: *rect, scale: 1.0 })
                        .collect(),
                },
            );
        }
        let seen = self.window.as_ref().map(|window| window.seen);
        let mut shell = Windows {
            open: &mut self.open,
            blocked: &self.blocked,
        };
        let mut live = Live {
            store: &mut self.store,
            world: &mut self.world,
        };
        let outcome = apply::frame(&ctx, &mut live, seen, &mut shell);
        self.saved |= outcome.save;
        self.preferences |= outcome.preferences;
        if let Some(window) = &mut self.window {
            window.send(&outcome.commands);
            window.frame();
        }
        self.time += 1.0 / 60.0;
        let screen = Rect::from_min_size(Pos2::ZERO, self.size);
        let open = self.open.clone();
        let menu = self.menu;
        let hidden = self.hidden;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                events: std::mem::take(&mut self.events),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                let p = crate::theme::Theme::default().palette();
                crate::ui::layout::set_work_area(ctx, screen);
                crate::ui::layout::set_move_area(ctx, screen);
                // as the interface's pass does: the card and the switcher,
                // and the menu when open
                super::interface(ctx, &p, hidden);
                if menu {
                    egui::Area::new(egui::Id::unique("test-world-menu"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(pos2(4.0, 300.0))
                        .show(ctx, |ui| super::menu::body(ui, &p));
                }
                for (id, at, size) in [
                    (INVENTORY, pos2(900.0, 56.0), vec2(360.0, 420.0)),
                    (PEOPLE, pos2(20.0, 60.0), vec2(300.0, 400.0)),
                    (MAP, pos2(340.0, 60.0), vec2(500.0, 400.0)),
                    (PREFERENCES, pos2(200.0, 300.0), vec2(400.0, 300.0)),
                ] {
                    if open.contains(id) {
                        Floater::new(id, id, at, size).show(ctx, &p, &mut true, |ui| ui.label("…"));
                    }
                }
                if open.contains(MINIMAP) {
                    let default = Rect::from_min_size(pos2(1000.0, 500.0), Vec2::splat(200.0));
                    widgets::bare_window(
                        ctx,
                        &p,
                        MINIMAP,
                        "Mini-map",
                        egui::Frame::new(),
                        default,
                        Vec2::splat(64.0),
                        false,
                        |ui| {
                            ui.label("…");
                        },
                    );
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
    }

    fn shows(&self, text: &str) -> bool {
        self.texts.iter().any(|(shown, _)| shown == text)
    }

    /// Clicks the text `label` of the card or of the menu.
    fn click(&mut self, label: &str) {
        let at = self
            .texts
            .iter()
            .find(|(shown, _)| shown == label)
            .unwrap_or_else(|| panic!("“{label}” is shown"))
            .1
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
            Vec::new(),
        ] {
            self.events = events;
            self.frame();
        }
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.frame();
        }
    }

    /// Enough frames for an apply to place its windows and settle, and for
    /// the next look at the windows.
    fn settle(&mut self) {
        self.frames(50);
    }

    fn rect(&self, id: &str) -> Rect {
        self.ctx.memory(|m| m.area_rect(egui::Id::unique(id))).expect("shown")
    }

    /// The user's hand: a floater resized, then moved (Aurora keeps a
    /// window inside the interface with the size it had before).
    fn put(&mut self, id: &str, x: f32, y: f32, width: f32, height: f32) {
        widgets::place_floater(&self.ctx, id, pos2(x, y), vec2(width, height));
        if self.open.contains(id) {
            self.frames(2);
            widgets::move_floater(&self.ctx, id, pos2(x, y));
            self.frames(2);
        }
    }

    fn save_new(&mut self, name: &str, parts: Parts) -> Result<usize, StoreError> {
        let ctx = self.ctx.clone();
        apply::save_new(&ctx, &mut self.live(), name, parts)
    }

    fn apply(&mut self, name: &str) -> apply::Report {
        let ctx = self.ctx.clone();
        let (index, _) = self.store.find(name).expect("in the list");
        apply::apply(&ctx, &mut self.live(), index).expect("applied")
    }

    fn previous(&mut self) -> bool {
        let ctx = self.ctx.clone();
        apply::previous(&ctx, &mut self.live())
    }

    fn modified(&self) -> bool {
        apply::modified(&self.store, &self.world)
    }

    fn layout(&self, name: &str) -> &Layout {
        self.store.find(name).expect("in the list").1
    }
}

fn close(a: Rect, b: Rect) -> bool {
    (a.min - b.min).abs().max_elem() <= 1.0 && (a.size() - b.size()).abs().max_elem() <= 1.0
}

/// A viewer with two layouts of the same windows: "Wide" (the world on the
/// right, the bars around it, windows on the left) and "Single".
fn two_layouts() -> Viewer {
    let mut v = Viewer::new(1600.0, 900.0);
    v.frames(4);
    v.world = ViewportSettings {
        enabled: true,
        left: 800,
        bars_around: true,
        ..Default::default()
    };
    v.put(INVENTORY, 10.0, 50.0, 500.0, 600.0);
    v.put(PEOPLE, 520.0, 50.0, 260.0, 400.0);
    v.put(MAP, 10.0, 660.0, 400.0, 230.0);
    v.frames(4);
    v.save_new("Wide", Parts::default()).expect("saved");
    v.world = ViewportSettings::default();
    v.put(INVENTORY, 1200.0, 60.0, 380.0, 500.0);
    v.put(PEOPLE, 20.0, 60.0, 300.0, 420.0);
    v.put(MAP, 400.0, 200.0, 600.0, 500.0);
    v.frames(4);
    v.save_new("Single", Parts::default()).expect("saved");
    v.settle();
    v
}

#[test]
fn a_layout_captures_the_allowlisted_windows_and_only_them() {
    let v = two_layouts();
    let wide = v.layout("Wide");
    let windows = wide.windows.as_ref().expect("the windows part");
    assert_eq!(windows.frame.width, 1600.0);
    assert_eq!(windows.frame.height, 900.0);
    let ids: Vec<&str> = windows.windows.keys().map(String::as_str).collect();
    assert_eq!(ids, [INVENTORY, MINIMAP, PEOPLE, MAP], "Preferences is never captured");
    let inventory = windows.windows[INVENTORY];
    assert_eq!(
        (inventory.x, inventory.y, inventory.width, inventory.height, inventory.minimized),
        (10.0, 50.0, 500.0, 600.0, false)
    );
    assert_eq!(
        wide.world.as_ref().map(|w| (w.enabled, w.left, w.bars_around)),
        Some((true, 800, true))
    );
    assert_eq!(
        v.store.active_layout.as_deref(),
        Some("Single"),
        "the last one saved is the active one"
    );
    assert!(!v.modified());
}

#[test]
fn applying_a_layout_puts_the_world_and_the_windows_back() {
    let mut v = two_layouts();
    let single = [v.rect(INVENTORY), v.rect(PEOPLE), v.rect(MAP), v.rect(MINIMAP)];
    let preferences = v.rect(PREFERENCES);
    let report = v.apply("Wide");
    assert_eq!(
        (report.name.as_deref(), report.windows, report.fitted, report.unknown),
        (Some("Wide"), 4, 0, 0)
    );
    // the world at once, the windows from the next frame
    assert!(v.world.enabled && v.world.left == 800 && v.world.bars_around);
    v.settle();
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(10.0, 50.0), vec2(500.0, 600.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
    assert!(
        close(v.rect(PEOPLE), Rect::from_min_size(pos2(520.0, 50.0), vec2(260.0, 400.0))),
        "{:?}",
        v.rect(PEOPLE)
    );
    assert!(
        close(v.rect(MAP), Rect::from_min_size(pos2(10.0, 660.0), vec2(400.0, 230.0))),
        "{:?}",
        v.rect(MAP)
    );
    assert_eq!(v.rect(PREFERENCES), preferences, "a window that is not on the list is not moved");
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert!(!v.modified(), "just applied");
    // and back: a window made small in between gets its size again
    v.put(INVENTORY, 10.0, 50.0, 200.0, 150.0);
    v.frames(3);
    v.apply("Single");
    v.settle();
    for (id, was) in [INVENTORY, PEOPLE, MAP, MINIMAP].into_iter().zip(single) {
        assert!(close(v.rect(id), was), "{id}: {:?} for {was:?}", v.rect(id));
    }
    assert_eq!(v.world, ViewportSettings::default());
    assert!(!v.modified());
}

#[test]
fn the_mini_map_takes_the_place_a_layout_gives_it() {
    let mut v = Viewer::new(1600.0, 900.0);
    v.frames(4);
    let default = v.rect(MINIMAP);
    // a layout that puts it elsewhere, at another size
    let mut windows = capture::windows(&v.ctx).expect("windows");
    windows.windows.insert(
        MINIMAP.into(),
        Placement {
            x: 40.0,
            y: 620.0,
            width: 260.0,
            height: 240.0,
            minimized: false,
            open: false,
        },
    );
    v.store
        .add(Layout::new("Map corner", None, Some(windows), Vec::new()))
        .expect("added");
    v.apply("Map corner");
    v.settle();
    let placed = v.rect(MINIMAP);
    assert_ne!(placed, default);
    assert!(
        close(placed, Rect::from_min_size(pos2(40.0, 620.0), vec2(260.0, 240.0))),
        "{placed:?}"
    );
    // it stays there: the request was taken once
    v.frames(5);
    assert_eq!(v.rect(MINIMAP), placed);
    assert!(apply::take_bare(&v.ctx, egui::Id::unique(MINIMAP)).is_none());
}

/// As if the window `id` had been closed when the layout `name` was saved
/// (or the layout saved before layouts knew which windows were open).
fn saved_closed(v: &mut Viewer, name: &str, id: &str) {
    for record in &mut v.store.layouts {
        if let super::store::Record::Layout(layout) = record
            && layout.name == name
            && let Some(place) = layout.windows.as_mut().and_then(|windows| windows.windows.get_mut(id))
        {
            place.open = false;
        }
    }
}

#[test]
fn a_closed_window_takes_its_place_when_it_opens() {
    let mut v = two_layouts();
    saved_closed(&mut v, "Wide", MAP);
    v.open.remove(MAP);
    v.frames(3);
    v.saved = false;
    v.apply("Wide");
    v.settle();
    assert_eq!(
        v.store.pending.iter().map(String::as_str).collect::<Vec<_>>(),
        [MAP],
        "it waits for its place"
    );
    assert!(v.saved, "and the settings are saved with it");
    assert!(!v.modified());
    // opened later: where "Wide" has it
    v.open.insert(MAP);
    v.settle();
    assert!(
        close(v.rect(MAP), Rect::from_min_size(pos2(10.0, 660.0), vec2(400.0, 230.0))),
        "{:?}",
        v.rect(MAP)
    );
    v.settle();
    assert!(v.store.pending.is_empty(), "no longer waiting");
    assert!(!v.modified(), "taking its place is not a change");
}

#[test]
fn a_window_still_waiting_at_quit_is_asked_again_at_the_next_start() {
    let mut v = two_layouts();
    saved_closed(&mut v, "Wide", MAP);
    v.open.remove(MAP);
    v.frames(3);
    v.apply("Wide");
    v.settle();
    assert!(v.store.pending.contains(MAP));
    // quit and start: egui's memory of the request is gone, the store's is not
    let mut next = v.restarted();
    next.settle();
    assert!(next.store.pending.contains(MAP), "still closed");
    next.open.insert(MAP);
    next.settle();
    assert!(
        close(next.rect(MAP), Rect::from_min_size(pos2(10.0, 660.0), vec2(400.0, 230.0))),
        "{:?}",
        next.rect(MAP)
    );
    next.settle();
    assert!(next.store.pending.is_empty());
}

#[test]
fn the_minimized_state_is_captured_with_the_expanded_size_and_applied() {
    let mut v = two_layouts();
    widgets::minimize_floater(&v.ctx, PEOPLE);
    v.frames(4);
    assert!(v.rect(PEOPLE).height() < 60.0, "a title bar");
    v.save_new("People small", Parts::default()).expect("saved");
    let people = v.layout("People small").windows.as_ref().expect("windows").windows[PEOPLE];
    assert!(people.minimized);
    assert_eq!((people.width, people.height), (300.0, 420.0), "the size it has when expanded");
    // "Single" has it expanded
    v.apply("Single");
    v.settle();
    assert!(!widgets::floater_minimized(&v.ctx, PEOPLE));
    assert!(
        close(v.rect(PEOPLE), Rect::from_min_size(pos2(20.0, 60.0), vec2(300.0, 420.0))),
        "{:?}",
        v.rect(PEOPLE)
    );
    // and back to minimized, at its bar's place
    v.apply("People small");
    v.settle();
    assert!(widgets::floater_minimized(&v.ctx, PEOPLE));
    assert!(v.rect(PEOPLE).height() < 60.0);
    assert!((v.rect(PEOPLE).min - pos2(people.x, people.y)).abs().max_elem() <= 1.0);
    // expanded by hand, it has its size
    widgets::expand_floater(&v.ctx, PEOPLE);
    v.frames(4);
    assert!(
        (v.rect(PEOPLE).size() - vec2(300.0, 420.0)).abs().max_elem() <= 1.0,
        "{:?}",
        v.rect(PEOPLE)
    );
}

/// Our reads of Aurora's entries for a floater (minimized, expanded size,
/// the placement request) agree with what `place_floater` writes: a key
/// renamed upstream fails here.
#[test]
fn our_reads_agree_with_aurora_s_placement_helpers() {
    let mut v = Viewer::new(1600.0, 900.0);
    v.open.remove(MAP);
    v.frames(4);
    let (people, map) = (windows::find(PEOPLE).expect("listed"), windows::find(MAP).expect("listed"));
    // a closed window, placed: the request waits
    v.put(MAP, 100.0, 120.0, 520.0, 380.0);
    v.frames(3);
    assert!(
        v.ctx
            .data(|d| d.get_temp::<egui::Pos2>(egui::Id::unique(MAP).with("place_pos")))
            .is_some()
    );
    // never shown, so no place of its own yet
    assert_eq!(capture::window(&v.ctx, map, Pos2::ZERO), None);
    // an open one takes it at once
    v.put(PEOPLE, 100.0, 120.0, 320.0, 380.0);
    v.frames(3);
    assert!(
        v.ctx
            .data(|d| d.get_temp::<egui::Pos2>(egui::Id::unique(PEOPLE).with("place_pos")))
            .is_none()
    );
    let placed = capture::window(&v.ctx, people, Pos2::ZERO).expect("known");
    assert_eq!(
        (placed.x, placed.y, placed.width, placed.height, placed.minimized),
        (100.0, 120.0, 320.0, 380.0, false)
    );
    // minimized: the expanded size is still read
    widgets::minimize_floater(&v.ctx, PEOPLE);
    v.frames(3);
    let minimized = capture::window(&v.ctx, people, Pos2::ZERO).expect("known");
    assert!(minimized.minimized);
    assert_eq!((minimized.width, minimized.height), (320.0, 380.0));
    // once opened, the closed one is where it was asked
    v.open.insert(MAP);
    v.frames(4);
    let opened = capture::window(&v.ctx, map, Pos2::ZERO).expect("known");
    assert_eq!((opened.x, opened.y, opened.width, opened.height), (100.0, 120.0, 520.0, 380.0));
}

#[test]
fn previous_swaps_with_the_arrangement_before_and_comes_back() {
    let mut v = two_layouts();
    assert!(!v.previous(), "nothing kept yet");
    // an arrangement that is no layout: Inventory moved by hand
    v.put(INVENTORY, 700.0, 300.0, 380.0, 500.0);
    v.settle();
    assert!(v.modified());
    let by_hand = v.rect(INVENTORY);
    v.apply("Wide");
    v.settle();
    let wide = v.rect(INVENTORY);
    assert_ne!(wide, by_hand);
    assert!(apply::has_previous(&v.ctx));
    // back to what was there, marks included
    assert!(v.previous());
    v.settle();
    assert!(close(v.rect(INVENTORY), by_hand), "{:?}", v.rect(INVENTORY));
    assert_eq!(v.world, ViewportSettings::default());
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert!(v.modified(), "it was modified then");
    // and again: "Wide"
    assert!(v.previous());
    v.settle();
    assert!(close(v.rect(INVENTORY), wide));
    assert!(v.world.enabled);
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert!(!v.modified());
}

#[test]
fn moving_a_window_or_the_world_marks_the_layout_modified_until_an_update() {
    let mut v = two_layouts();
    v.apply("Wide");
    v.settle();
    v.saved = false;
    assert!(!v.modified());
    // a window that is not on the list does not count
    v.put(PREFERENCES, 600.0, 100.0, 400.0, 300.0);
    v.settle();
    assert!(!v.modified() && !v.saved);
    // one point is nothing
    v.put(PEOPLE, 521.0, 51.0, 260.0, 400.0);
    v.settle();
    assert!(!v.modified());
    // a real move
    v.put(PEOPLE, 560.0, 80.0, 260.0, 400.0);
    v.settle();
    assert!(v.modified() && v.store.modified);
    assert!(v.saved, "the mark is saved");
    // Update: the layout takes the arrangement, the mark goes
    let ctx = v.ctx.clone();
    let (index, _) = v.store.find("Wide").expect("in the list");
    apply::update(&ctx, &mut v.live(), index).expect("updated");
    let people = v.layout("Wide").windows.as_ref().expect("windows").windows[PEOPLE];
    assert_eq!((people.x, people.y), (560.0, 80.0));
    v.settle();
    assert!(!v.modified());
    // the world part is compared with the settings themselves
    v.world.left = 700;
    assert!(v.modified());
    v.world.left = 800;
    assert!(!v.modified());
    // minimizing a window is a change too
    widgets::minimize_floater(&v.ctx, MAP);
    v.settle();
    assert!(v.modified());
}

#[test]
fn a_viewer_window_resized_does_not_mark_the_layout_modified() {
    let mut v = two_layouts();
    v.apply("Single");
    v.settle();
    // Aurora moves the windows with their anchors, and shrinks them
    v.size = vec2(1000.0, 700.0);
    v.settle();
    assert_ne!(v.rect(INVENTORY).min, pos2(1200.0, 60.0));
    assert!(!v.modified());
    v.size = vec2(1600.0, 900.0);
    v.settle();
    assert!(!v.modified());
    // a move after that still counts
    v.put(PEOPLE, 300.0, 300.0, 300.0, 420.0);
    v.settle();
    assert!(v.modified());
}

#[test]
fn a_layout_saved_on_a_larger_interface_is_fitted_and_never_rewritten() {
    let mut v = two_layouts();
    let saved = v.store.clone();
    // the viewer window shrunk from 1600 × 900 to 1000 × 700
    v.size = vec2(1000.0, 700.0);
    v.frames(6);
    let report = v.apply("Single");
    assert_eq!(report.windows, 4);
    assert!(report.fitted >= 2, "Inventory at the right edge, the map too wide: {report:?}");
    v.settle();
    let screen = Rect::from_min_size(Pos2::ZERO, v.size);
    for id in [INVENTORY, PEOPLE, MAP, MINIMAP] {
        assert!(screen.contains_rect(v.rect(id)), "{id}: {:?}", v.rect(id));
    }
    // Inventory keeps its 20 points from the right edge, and its size
    let inventory = v.rect(INVENTORY);
    assert!(
        close(inventory, Rect::from_min_size(pos2(600.0, 60.0), vec2(380.0, 500.0))),
        "{inventory:?}"
    );
    assert!(!v.modified());
    // the stored layouts hold what was saved, to the last number
    let mut now = v.store.clone();
    now.active_layout = saved.active_layout.clone();
    assert_eq!(now.layouts, saved.layouts);
    // back on the large interface the windows have their saved sizes
    v.size = vec2(1600.0, 900.0);
    v.frames(6);
    v.apply("Single");
    v.settle();
    assert!(
        close(v.rect(MAP), Rect::from_min_size(pos2(400.0, 200.0), vec2(600.0, 500.0))),
        "{:?}",
        v.rect(MAP)
    );
}

#[test]
fn a_part_a_layout_does_not_have_is_left_alone() {
    let mut v = two_layouts();
    v.world = ViewportSettings {
        enabled: true,
        right: 500,
        ..Default::default()
    };
    v.save_new(
        "World only",
        Parts {
            world: true,
            windows: false,
            ..Default::default()
        },
    )
    .expect("saved");
    v.save_new(
        "Windows only",
        Parts {
            world: false,
            windows: true,
            ..Default::default()
        },
    )
    .expect("saved");
    assert_eq!(
        v.save_new(
            "Nothing",
            Parts {
                world: false,
                windows: false,
                ..Default::default()
            }
        ),
        Err(StoreError::Invalid)
    );
    assert!(v.layout("World only").windows.is_none() && v.layout("Windows only").world.is_none());
    v.apply("Wide");
    v.settle();
    let wide = [v.rect(INVENTORY), v.rect(PEOPLE), v.rect(MAP)];
    // the world changes, the windows stay
    v.apply("World only");
    v.settle();
    assert_eq!(v.world.right, 500);
    assert_eq!([v.rect(INVENTORY), v.rect(PEOPLE), v.rect(MAP)], wide);
    // moving a window does not mark a layout without windows
    v.put(PEOPLE, 560.0, 80.0, 260.0, 400.0);
    v.settle();
    assert!(!v.modified());
    // the windows change, the world stays
    v.apply("Windows only");
    v.settle();
    assert_eq!(v.world.right, 500);
    assert!(close(
        v.rect(INVENTORY),
        Rect::from_min_size(pos2(1200.0, 60.0), vec2(380.0, 500.0))
    ));
    assert!(!v.modified(), "its absent world part is not compared");
    v.world.right = 400;
    assert!(!v.modified());
}

#[test]
fn a_new_apply_replaces_one_still_waiting_for_its_frame() {
    let mut v = two_layouts();
    v.apply("Wide");
    // before a frame passes
    v.apply("Single");
    v.settle();
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(1200.0, 60.0), vec2(380.0, 500.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
    assert_eq!(v.world, ViewportSettings::default());
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert!(!v.modified());
}

#[test]
fn windows_this_version_does_not_know_are_counted_and_left_alone() {
    let mut v = two_layouts();
    let mut windows: Placements = v.layout("Single").windows.clone().expect("windows");
    let place = windows.windows[INVENTORY];
    // a stored string never moves a window that is not on the list
    windows.windows.insert(PREFERENCES.into(), Placement { x: 0.0, y: 0.0, ..place });
    windows.windows.insert("a_window_of_later".into(), place);
    v.store
        .add(Layout::new("From later", None, Some(windows), Vec::new()))
        .expect("added");
    let preferences = v.rect(PREFERENCES);
    let report = v.apply("From later");
    assert_eq!((report.windows, report.unknown), (4, 2));
    v.settle();
    assert_eq!(v.rect(PREFERENCES), preferences);
    assert!(v.store.pending.is_empty(), "nothing waits for a window that does not exist");
}

#[test]
fn reset_bars_and_world_leaves_the_windows_and_can_be_undone() {
    let mut v = two_layouts();
    v.apply("Wide");
    v.settle();
    let wide = [v.rect(INVENTORY), v.rect(PEOPLE), v.rect(MAP)];
    let ctx = v.ctx.clone();
    assert!(apply::reset_world(&ctx, &mut v.live()));
    v.settle();
    assert_eq!(v.world, ViewportSettings::default());
    assert_eq!([v.rect(INVENTORY), v.rect(PEOPLE), v.rect(MAP)], wide);
    assert!(v.modified(), "the world is no longer the layout's");
    assert!(!apply::reset_world(&ctx, &mut v.live()), "nothing left to reset");
    assert!(v.previous());
    assert!(v.world.enabled && v.world.bars_around);
    assert!(!v.modified());
}

#[test]
fn an_interface_too_small_to_hold_windows_is_not_captured() {
    let mut v = Viewer::new(200.0, 100.0);
    v.frames(3);
    assert_eq!(capture::room(&v.ctx), None);
    assert_eq!(capture::windows(&v.ctx), None);
    assert_eq!(v.save_new("Tiny", Parts::default()), Err(StoreError::Invalid));
    // a layout of the world alone can still be saved
    assert!(
        v.save_new(
            "World",
            Parts {
                world: true,
                windows: false,
                ..Default::default()
            }
        )
        .is_ok()
    );
}
