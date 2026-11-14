//! Frame tests of checkpoint 5: the switcher, the toolbar's two commands,
//! the keyboard's actions, the favorites in World › Layouts, and the card
//! while the interface is hidden. The same pretend viewer as the others.

use super::*;
use crate::folderstorm::keys::Key;
use crate::folderstorm::workspaces::apply::Notice;
use crate::folderstorm::workspaces::menu::{self, Request};
use crate::folderstorm::workspaces::store::{MonitorPolicy, WindowFrame};
use crate::folderstorm::workspaces::window::Pretend;
use crate::folderstorm::workspaces::{switcher, toolbar};
use aurora_i18n::tr;

impl Viewer {
    /// Something asked as the menu, a button or a key would, and the
    /// frames for it to be run and drawn.
    pub(super) fn ask(&mut self, request: Request) {
        menu::ask(&self.ctx, request);
        self.frames(3);
    }

    pub(super) fn key(&mut self, key: egui::Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        for events in [vec![event(true)], vec![event(false)], Vec::new()] {
            self.events = events;
            self.frame();
        }
    }

    pub(super) fn type_text(&mut self, text: &str) {
        self.events = vec![egui::Event::Text(text.to_owned())];
        self.frames(2);
    }

    pub(super) fn click_at(&mut self, at: Pos2) {
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

    /// Where the first text `label` was drawn.
    pub(super) fn text_rect(&self, label: &str) -> Rect {
        self.texts
            .iter()
            .find(|(shown, _)| shown == label)
            .unwrap_or_else(|| panic!("“{label}” is shown"))
            .1
    }

    /// Clicks the star of the switcher's line of `name`.
    pub(super) fn click_star(&mut self, name: &str) {
        let at = self.text_rect(name);
        self.click_at(pos2(at.left() - 16.0, at.center().y));
    }

    fn active(&self) -> Option<&str> {
        self.store.active_layout.as_deref()
    }

    pub(super) fn active_layout(&self) -> Option<&str> {
        self.active()
    }

    pub(super) fn open_switcher(&mut self) {
        self.ask(Request::Switcher);
        assert!(switcher::is_open(&self.ctx));
    }

    /// The names the switcher lists, top down (each is drawn once in the
    /// list; the selected one again beside the picture).
    fn listed(&self) -> Vec<String> {
        let mut names: Vec<(f32, String)> = Vec::new();
        for (text, rect) in &self.texts {
            if self.store.find(text).is_some() && !names.iter().any(|(_, name)| name == text) {
                names.push((rect.top(), text.clone()));
            }
        }
        names.sort_by(|a, b| a.0.total_cmp(&b.0));
        names.into_iter().map(|(_, name)| name).collect()
    }
}

/// "Wide", "Single" (in use) and "Photos", as [`two_layouts`] leaves them.
fn three_layouts() -> Viewer {
    let mut v = two_layouts();
    v.save_new("Photos", Parts::default()).expect("saved");
    v.apply("Single");
    v.settle();
    v.saved = false;
    v
}

#[test]
fn the_switcher_opens_with_the_keyboard_in_its_field() {
    let mut v = two_layouts();
    assert!(!switcher::is_open(&v.ctx));
    v.open_switcher();
    assert!(v.shows(tr!("fs-switcher-title")));
    assert_eq!(v.ctx.memory(|m| m.focused()), Some(switcher::field_id()), "type at once");
    assert_eq!(v.listed(), ["Wide", "Single"]);
    assert!(v.shows(tr!("fs-layouts-active")), "the layout in use is marked");
    assert!(!v.shows(&tr!("fs-switcher-favorites").to_uppercase()), "no favorite: no headings");
    // asked again: put away
    v.ask(Request::Switcher);
    assert!(!switcher::is_open(&v.ctx));
    assert!(!v.shows(tr!("fs-switcher-title")));
    assert!(!v.saved, "looking changes nothing");
}

#[test]
fn enter_applies_the_other_layout_and_closes_the_window() {
    let mut v = two_layouts();
    assert_eq!(v.active(), Some("Single"));
    v.open_switcher();
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Wide"), "the first that is not in use");
    assert!(!switcher::is_open(&v.ctx), "Enter closes");
    assert!(v.saved);
    assert!(v.world.enabled, "applied as from the list");
    // and back the same way
    v.open_switcher();
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Single"));
    assert!(!v.world.enabled);
}

#[test]
fn typing_keeps_the_layouts_that_have_it_and_enter_applies_the_first() {
    let mut v = three_layouts();
    v.open_switcher();
    assert_eq!(v.listed(), ["Wide", "Single", "Photos"]);
    v.type_text("s");
    assert_eq!(v.listed(), ["Single", "Photos"], "the names with an s, in their order");
    v.type_text("in");
    assert_eq!(v.listed(), ["Single"]);
    // nothing has it: said, and Enter does nothing
    v.type_text("x");
    assert!(v.listed().is_empty());
    assert!(v.shows(tr!("fs-switcher-no-match")));
    v.key(egui::Key::Enter);
    assert!(switcher::is_open(&v.ctx));
    assert!(!v.saved);
    // a new switcher starts with nothing typed
    v.ask(Request::Switcher);
    v.open_switcher();
    assert_eq!(v.listed(), ["Wide", "Single", "Photos"]);
    v.type_text("PHO");
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Photos"), "whatever the case");
    assert!(!switcher::is_open(&v.ctx));
}

#[test]
fn the_arrows_choose_and_go_round() {
    let mut v = three_layouts();
    v.open_switcher();
    // starts on "Wide" (the first not in use): down twice is "Photos"
    v.key(egui::Key::ArrowDown);
    v.key(egui::Key::ArrowDown);
    assert_eq!(
        v.ctx.memory(|m| m.focused()),
        Some(switcher::field_id()),
        "the keyboard stays in the field"
    );
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Photos"));
    // from "Wide" again: up goes round to the last, down comes back
    v.open_switcher();
    v.key(egui::Key::ArrowUp);
    v.key(egui::Key::ArrowDown);
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Wide"));
}

#[test]
fn escape_closes_and_applies_nothing() {
    let mut v = two_layouts();
    v.open_switcher();
    v.type_text("wi");
    v.key(egui::Key::Escape);
    assert!(!switcher::is_open(&v.ctx));
    assert_eq!(v.active(), Some("Single"));
    assert!(!v.saved);
}

#[test]
fn a_click_selects_and_a_double_click_applies_without_closing() {
    let mut v = three_layouts();
    v.open_switcher();
    v.click("Photos");
    assert!(!v.saved, "selected only");
    v.click(tr!("fs-layouts-apply"));
    v.settle();
    assert_eq!(v.active(), Some("Photos"));
    assert!(switcher::is_open(&v.ctx), "the button leaves the window open");
    assert!(v.shows(&crate::folderstorm::workspaces::words::report_text(
        &apply::report(&v.ctx).expect("a report"),
        v.store.pending.len()
    )));
}

#[test]
fn a_star_makes_a_favorite_first_in_the_list_and_in_the_menu() {
    let mut v = three_layouts();
    v.menu = true;
    v.open_switcher();
    v.click_star("Photos");
    assert!(v.store.is_favorite("Photos"));
    assert!(v.saved);
    assert_eq!(v.listed(), ["Photos", "Wide", "Single"]);
    assert!(v.shows(&tr!("fs-switcher-favorites").to_uppercase()));
    assert!(v.shows(&tr!("fs-switcher-layouts").to_uppercase()));
    v.click_star("Single");
    assert_eq!(v.listed(), ["Photos", "Single", "Wide"], "in the order they were starred");
    // World › Layouts lists the favorites alone
    v.ask(Request::Switcher);
    assert_eq!(v.listed(), ["Photos", "Single"]);
    // the star again takes it away
    v.open_switcher();
    v.click_star("Photos");
    v.click_star("Single");
    assert!(v.store.favorites.is_empty());
    v.ask(Request::Switcher);
    assert_eq!(v.listed(), ["Wide", "Single", "Photos"], "no favorite: every layout");
}

#[test]
fn save_new_takes_a_name_and_says_why_it_refuses_one() {
    let mut v = two_layouts();
    v.open_switcher();
    v.click(tr!("fs-switcher-save-new"));
    assert!(v.shows(tr!("fs-layouts-part-world")) && v.shows(tr!("fs-layouts-part-windows")));
    // a name already taken: said at once, nothing saved
    v.type_text("single");
    v.key(egui::Key::Enter);
    assert!(v.shows(tr!("fs-layouts-error-taken")));
    assert_eq!(v.store.layouts.len(), 2);
    // corrected: saved, in use, the form put away
    v.click(tr!("common-cancel"));
    assert_eq!(v.store.layouts.len(), 2);
    v.click(tr!("fs-switcher-save-new"));
    v.type_text("Evening");
    v.key(egui::Key::Enter);
    v.frames(3);
    assert_eq!(v.store.layouts.len(), 3);
    assert_eq!(v.active(), Some("Evening"));
    assert!(v.saved);
    let saved = v.layout("Evening");
    assert!(saved.world.is_some() && saved.windows.is_some());
    assert!(!v.shows(tr!("fs-layouts-part-windows")), "the form is closed");
    assert_eq!(v.listed(), ["Wide", "Single", "Evening"]);
}

#[test]
fn update_asks_then_saves_the_arrangement_in_the_layout_in_use() {
    let mut v = two_layouts();
    v.put(INVENTORY, 700.0, 100.0, 420.0, 480.0);
    v.settle();
    assert!(v.modified());
    v.open_switcher();
    assert!(v.shows(tr!("fs-layouts-modified")));
    v.click(&tr!("fs-switcher-update", name = "Single"));
    assert!(v.shows(&tr!("fs-layouts-ask-update", name = "Single")), "asks first, naming it");
    assert!(v.modified(), "nothing saved yet");
    v.click(tr!("common-cancel"));
    assert!(v.modified() && !v.shows(&tr!("fs-layouts-ask-update", name = "Single")));
    v.click(&tr!("fs-switcher-update", name = "Single"));
    v.click(tr!("fs-layouts-update"));
    v.settle();
    assert!(!v.modified());
    let inventory = v.layout("Single").windows.as_ref().expect("its windows").windows[INVENTORY];
    assert_eq!((inventory.x, inventory.y), (700.0, 100.0));
}

#[test]
fn previous_and_preferences_are_in_the_footer() {
    let mut v = two_layouts();
    v.apply("Wide");
    v.settle();
    v.open_switcher();
    v.click(tr!("fs-layouts-previous"));
    v.settle();
    assert_eq!(v.active(), Some("Single"));
    assert!(!v.preferences);
    v.click(tr!("common-preferences"));
    assert!(v.preferences);
}

#[test]
fn the_toolbar_button_shows_the_switcher_and_is_lit_while_it_is_open() {
    let mut v = two_layouts();
    let mut panels = crate::ui::Panels::from_settings(&crate::settings::Settings::default());
    let before = format!("{panels:?}");
    let look = |v: &Viewer| toolbar::look(&v.ctx, toolbar::SWITCHER, false, true, "Layouts".to_owned());
    assert_eq!(look(&v), (false, true, "Layouts".to_owned()));
    let action = crate::ui::bars::run_command(&v.ctx, toolbar::SWITCHER, &mut panels);
    assert!(matches!(action, crate::ui::bars::BarAction::None));
    assert_eq!(format!("{panels:?}"), before, "no window of Aurora's is touched");
    v.frames(3);
    assert!(switcher::is_open(&v.ctx));
    assert!(look(&v).0, "lit");
    crate::ui::bars::run_command(&v.ctx, toolbar::SWITCHER, &mut panels);
    v.frames(3);
    assert!(!switcher::is_open(&v.ctx));
    assert!(!look(&v).0);
    // another command's look is given back as it came
    assert_eq!(
        toolbar::look(&v.ctx, "inventory", true, false, "tip".to_owned()),
        (true, false, "tip".to_owned())
    );
    assert!(toolbar::own(toolbar::SWITCHER) && toolbar::own(toolbar::TOGGLE) && !toolbar::own("inventory"));
}

#[test]
fn the_toggle_button_goes_from_one_of_its_two_layouts_to_the_other() {
    let mut v = two_layouts();
    let mut panels = crate::ui::Panels::from_settings(&crate::settings::Settings::default());
    let look = |v: &Viewer| toolbar::look(&v.ctx, toolbar::TOGGLE, false, true, "Switch layout".to_owned());
    // nothing chosen: dimmed, says where to choose, and a click opens it
    let (lit, usable, tip) = look(&v);
    assert!(!lit && !usable);
    assert!(tip.starts_with("Switch layout\n") && tip.ends_with(tr!("fs-switcher-toggle-unset")));
    crate::ui::bars::run_command(&v.ctx, toolbar::TOGGLE, &mut panels);
    v.frames(3);
    assert!(v.preferences, "Preferences, where the two are chosen");
    assert_eq!(v.active(), Some("Single"));
    // chosen: the tooltip names the one it goes to
    v.store.set_toggle(0, Some("Wide"));
    v.store.set_toggle(1, Some("Single"));
    v.preferences = false;
    v.frames(2);
    assert_eq!(look(&v), (false, true, tr!("fs-switcher-toggle-to", name = "Wide")));
    crate::ui::bars::run_command(&v.ctx, toolbar::TOGGLE, &mut panels);
    v.settle();
    assert_eq!(v.active(), Some("Wide"));
    assert!(v.world.enabled && !v.preferences);
    assert_eq!(look(&v).2, tr!("fs-switcher-toggle-to", name = "Single"));
    crate::ui::bars::run_command(&v.ctx, toolbar::TOGGLE, &mut panels);
    v.settle();
    assert_eq!(v.active(), Some("Single"));
    // *Previous arrangement* undoes a toggle like any apply
    assert!(v.previous());
    v.settle();
    assert_eq!(v.active(), Some("Wide"));
    // one of the two deleted: dimmed again
    let (index, _) = v.store.find("Single").expect("in the list");
    v.store.remove(index);
    v.frames(2);
    assert!(!look(&v).1);
}

#[test]
fn each_key_asks_what_its_button_would() {
    assert_eq!(menu::key_request(Key::LayoutSwitcher, false), Some(Request::Switcher));
    assert_eq!(menu::key_request(Key::LayoutToggle, false), Some(Request::Toggle));
    assert_eq!(menu::key_request(Key::PreviousArrangement, false), Some(Request::Previous));
    assert_eq!(menu::key_request(Key::NextFavoriteLayout, false), Some(Request::Favorite(true)));
    assert_eq!(
        menu::key_request(Key::PreviousFavoriteLayout, false),
        Some(Request::Favorite(false))
    );
    // in mouselook the switcher's window could not be used; the rest applies
    assert_eq!(menu::key_request(Key::LayoutSwitcher, true), None);
    for key in crate::folderstorm::keys::KEYS {
        assert_eq!(menu::key_request(key, true).is_some(), key != Key::LayoutSwitcher, "{key:?}");
    }
}

#[test]
fn the_favorite_keys_go_round_the_favorites() {
    let mut v = three_layouts();
    // no favorite: nothing happens
    v.ask(Request::Favorite(true));
    assert_eq!(v.active(), Some("Single"));
    assert!(!v.saved);
    v.store.set_favorite("Photos", true);
    v.store.set_favorite("Wide", true);
    let mut round = Vec::new();
    for _ in 0..3 {
        v.ask(Request::Favorite(true));
        v.settle();
        round.push(v.active().expect("one in use").to_owned());
    }
    assert_eq!(
        round,
        ["Photos", "Wide", "Photos"],
        "from a layout that is not a favorite: the first"
    );
    v.ask(Request::Favorite(false));
    v.settle();
    assert_eq!(v.active(), Some("Wide"));
    // the keys of the toggle and of Previous, through the same requests
    v.store.set_toggle(0, Some("Single"));
    v.store.set_toggle(1, Some("Wide"));
    v.ask(Request::Toggle);
    v.settle();
    assert_eq!(v.active(), Some("Single"));
    v.ask(Request::Previous);
    v.settle();
    assert_eq!(v.active(), Some("Wide"));
}

#[test]
fn the_switcher_is_never_captured_nor_moved_by_a_layout() {
    let mut v = two_layouts();
    assert!(windows::find(switcher::WINDOW).is_none(), "not on the allowlist");
    v.open_switcher();
    let before = v.rect(switcher::WINDOW);
    v.save_new("With the switcher open", Parts::default()).expect("saved");
    let saved = v.layout("With the switcher open").windows.as_ref().expect("its windows");
    assert!(!saved.windows.contains_key(switcher::WINDOW));
    v.apply("Wide");
    v.settle();
    assert_eq!(v.rect(switcher::WINDOW), before, "left where the user has it");
    assert!(switcher::is_open(&v.ctx));
}

#[test]
fn the_switcher_follows_the_store_while_it_is_open() {
    let mut v = two_layouts();
    v.open_switcher();
    // changed elsewhere (Preferences): the list follows
    let (index, _) = v.store.find("Wide").expect("in the list");
    v.store.rename(index, "Both screens").expect("renamed");
    v.frames(2);
    assert_eq!(v.listed(), ["Both screens", "Single"]);
    v.store.remove(index);
    v.frames(2);
    assert_eq!(v.listed(), ["Single"]);
    v.key(egui::Key::Enter);
    v.settle();
    assert_eq!(v.active(), Some("Single"), "a single layout, in use: applied again");
}

/// The card is not drawn while it could not be clicked (mouselook, the
/// interface put away), and waits: one that only tells counts its time
/// from when it shows.
#[test]
fn the_card_waits_while_the_interface_is_hidden() {
    let mut v = Viewer::new(1920.0, 1040.0);
    v.window = Some(Pretend::new(0, 0, 1920, 1040));
    v.monitors = vec![[0, 0, 1920, 1080]];
    v.frames(4);
    v.save_new("One", Parts::default()).expect("saved");
    v.frames(200);
    let frame = WindowFrame::Normal {
        x: Some(0),
        y: Some(0),
        width: 3840,
        height: 1040,
        maximized: false,
        locked: false,
    };
    let monitor = |x: i32| crate::folderstorm::workspaces::store::Monitor {
        x,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    v.store
        .add(Layout::new("Two", None, None, vec![monitor(0), monitor(1920)]).with_window(Some(frame)))
        .expect("added");
    v.apply("One");
    v.store.on_monitor_change = MonitorPolicy::Apply;
    v.settle();
    // in mouselook, a second monitor is plugged in: "Two" is applied, and
    // the card that tells it is kept back
    v.hidden = true;
    v.monitors = vec![[0, 0, 1920, 1080], [1920, 0, 1920, 1080]];
    v.frames(260);
    assert_eq!(v.active(), Some("Two"));
    let told = tr!("fs-layouts-monitors-applied", name = "Two");
    assert_eq!(apply::notice(&v.ctx), Some(Notice::Applied { name: "Two".to_owned() }));
    assert!(!v.shows(&told), "not drawn where it cannot be clicked");
    // long after, still waiting
    v.frames(15 * 60);
    assert!(apply::notice(&v.ctx).is_some());
    // the interface is back: the card shows, for its twelve seconds
    v.hidden = false;
    v.frames(3);
    assert!(v.shows(&told));
    v.frames(10 * 60);
    assert!(v.shows(&told));
    v.frames(3 * 60);
    assert!(!v.shows(&told) && apply::notice(&v.ctx).is_none());
}

/// The switcher drawn with the software painter over a viewer on the
/// left one of two monitors, "2 monitors" selected: its picture shows the
/// window over both. `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the
/// pictures as `<folder>/switcher.png`, `switcher-missing.png` and
/// `switcher-save.png`.
#[test]
fn the_switcher_is_drawn() {
    use crate::folderstorm::diagram::{self, Desktop, Monitor};
    use crate::folderstorm::workspaces::store::{Frame, Placement, Placements};
    let theme = crate::theme::Theme::default();
    let p = theme.skin_palette();
    let ctx = egui::Context::default();
    theme.apply(&ctx, 1.0);
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let _icons = crate::ui::icons::Icons::load(&ctx, None);
    let monitor = |x: i32| crate::folderstorm::workspaces::store::Monitor {
        x,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    let place = |x: f32, y: f32, width: f32, height: f32| Placement {
        x,
        y,
        width,
        height,
        minimized: false,
        open: false,
    };
    let wide_world = ViewportSettings {
        enabled: true,
        left: 1920,
        bars_around: true,
        ..Default::default()
    };
    let wide_windows = Placements {
        frame: Frame {
            width: 3840.0,
            height: 1040.0,
            pixels_per_point: 1.0,
        },
        windows: [
            (INVENTORY, place(10.0, 50.0, 700.0, 900.0)),
            (MAP, place(720.0, 50.0, 800.0, 560.0)),
            (PEOPLE, place(1530.0, 50.0, 380.0, 700.0)),
        ]
        .into_iter()
        .map(|(id, place)| (id.to_owned(), place))
        .collect(),
    };
    let wide_frame = WindowFrame::Normal {
        x: Some(0),
        y: Some(0),
        width: 3840,
        height: 1040,
        maximized: false,
        locked: true,
    };
    let single_windows = Placements {
        frame: Frame {
            width: 1280.0,
            height: 800.0,
            pixels_per_point: 1.0,
        },
        windows: [
            (INVENTORY, place(880.0, 60.0, 380.0, 500.0)),
            (PEOPLE, place(20.0, 60.0, 300.0, 420.0)),
        ]
        .into_iter()
        .map(|(id, place)| (id.to_owned(), place))
        .collect(),
    };
    let mut store = Store::default();
    for layout in [
        Layout::new("2 monitors", Some(wide_world), Some(wide_windows), vec![monitor(0), monitor(1920)]).with_window(Some(wide_frame)),
        Layout::new("1 monitor", Some(Default::default()), Some(single_windows), vec![monitor(0)]),
        Layout::new("Photos", Some(Default::default()), None, Vec::new()),
    ] {
        store.add(layout).expect("added");
    }
    store.set_active(Some("1 monitor".to_owned()));
    store.set_favorite("2 monitors", true);
    store.set_favorite("1 monitor", true);
    // two workspaces (checkpoint 6): after the favorites, before the layouts
    {
        use crate::folderstorm::workspaces::workspace::Workspace;
        let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        let sorting = Workspace::new("Sorting", Some("2 monitors"), ids(&[INVENTORY, windows::ADDITIONAL_INVENTORY, PEOPLE]));
        store.add_workspace(sorting).expect("added");
        store
            .add_workspace(Workspace::new("Exploring", None, ids(&[MINIMAP, MAP, "places"])))
            .expect("added");
    }
    let mut world = ViewportSettings::default();
    let mut window = Pretend::new(320, 140, 1280, 800);
    let (width, height) = (1280u32, 800u32);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32));
    let mut pixels = vec![0u32; (width * height) as usize];
    let mut painter = crate::crash::software::Painter::default();
    let mut texts: Vec<(String, Rect)> = Vec::new();
    let mut events: Vec<egui::Event> = Vec::new();
    let mut time = 0.0;
    // the second monitor is taken away for one of the pictures
    let unplugged = std::cell::Cell::new(false);
    let mut draw = |name: &str, frames: usize, events: &mut Vec<egui::Event>, store: &mut Store, texts: &mut Vec<(String, Rect)>| {
        for n in 0..frames {
            diagram::set_desktop(
                &ctx,
                Desktop {
                    window: window.seen.inner,
                    position: window.seen.outer,
                    monitors: [[0, 0, 1920, 1080], [1920, 0, 1920, 1080]]
                        .iter()
                        .take(if unplugged.get() { 1 } else { 2 })
                        .map(|rect| Monitor { rect: *rect, scale: 1.0 })
                        .collect(),
                },
            );
            let outcome = apply::frame(&ctx, &mut Live { store, world: &mut world }, Some(window.seen), &mut apply::NoShell);
            window.send(&outcome.commands);
            window.frame();
            time += 1.0 / 60.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    events: std::mem::take(events),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx();
                    crate::ui::layout::set_work_area(ctx, screen);
                    crate::ui::layout::set_move_area(ctx, screen);
                    crate::folderstorm::workspaces::interface(ctx, &p, false);
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                    egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                collect(&clipped.shape, texts);
            }
            // (the painter is given every frame's textures, and draws the last)
            let last = n + 1 == frames;
            let shapes = if last { std::mem::take(&mut output.shapes) } else { Vec::new() };
            let primitives = ctx.tessellate(shapes, output.pixels_per_point);
            if last {
                pixels.fill(0);
            }
            painter.paint(
                &mut pixels,
                [width as usize, height as usize],
                &primitives,
                &output.textures_delta,
                output.pixels_per_point,
                p.navy,
            );
            output.textures_delta.clear();
        }
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
    };
    draw("warm-up", 40, &mut events, &mut store, &mut texts);
    menu::ask(&ctx, Request::Switcher);
    draw("switcher", 6, &mut events, &mut store, &mut texts);
    let shown = |texts: &[(String, Rect)], text: &str| texts.iter().any(|(drawn, _)| drawn == text);
    for expected in [
        tr!("fs-switcher-title").to_owned(),
        tr!("fs-switcher-favorites").to_uppercase(),
        tr!("fs-switcher-workspaces").to_uppercase(),
        tr!("fs-switcher-layouts").to_uppercase(),
        "Sorting".to_owned(),
        "Exploring".to_owned(),
        "2 monitors".to_owned(),
        "1 monitor".to_owned(),
        "Photos".to_owned(),
        tr!("fs-layouts-active").to_owned(),
        tr!("fs-layouts-apply").to_owned(),
        tr!("fs-switcher-update", name = "1 monitor"),
        tr!("fs-switcher-save-new").to_owned(),
        tr!("fs-layouts-previous").to_owned(),
        tr!("common-preferences").to_owned(),
        tr!("fs-switcher-keys").to_owned(),
    ] {
        assert!(shown(&texts, &expected), "“{expected}” is drawn");
    }
    for icon in [
        switcher::ICON_STAR,
        switcher::ICON_NO_STAR,
        switcher::ICON_LOCK,
        switcher::ICON_APPLY,
        switcher::ICON_UPDATE,
        switcher::ICON_SAVE,
        switcher::ICON_CANCEL,
        switcher::ICON_PREFERENCES,
        menu::ICON_PREVIOUS,
        menu::ICON_UNLOCK,
        menu::ICON_WORKSPACE,
    ] {
        assert!(crate::ui::icons::global(icon).is_some(), "the icon “{icon}” is embedded");
    }
    // the same with the second monitor unplugged: it is drawn dashed, and
    // what will not be as saved is said (`switcher-missing.png`)
    unplugged.set(true);
    draw("switcher-missing", 6, &mut events, &mut store, &mut texts);
    for expected in [
        tr!("fs-switcher-note-missing", count = 1),
        tr!("fs-switcher-note-unlocked").to_owned(),
        tr!("fs-switcher-note-set-aside").to_owned(),
    ] {
        assert!(shown(&texts, &expected), "“{expected}” is drawn");
    }
    unplugged.set(false);
    // *Save new…*, with a name typed
    let at = texts
        .iter()
        .find(|(drawn, _)| drawn == tr!("fs-switcher-save-new"))
        .expect("the button")
        .1
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    for step in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
        events = step;
        draw("switcher-save", 1, &mut events, &mut store, &mut texts);
    }
    draw("switcher-save", 2, &mut events, &mut store, &mut texts);
    events = vec![egui::Event::Text("Evening".to_owned())];
    draw("switcher-save", 4, &mut events, &mut store, &mut texts);
    for expected in [
        tr!("fs-layouts-save"),
        tr!("common-cancel"),
        tr!("fs-layouts-part-window"),
        tr!("fs-layouts-part-world"),
        tr!("fs-layouts-part-windows"),
        tr!("fs-layouts-lock"),
    ] {
        assert!(shown(&texts, expected), "“{expected}” is drawn");
    }
    // a workspace selected: the picture of its layout, what it opens
    // (`switcher-workspace.png`), then *Save new…* as a workspace
    // (`switcher-save-workspace.png`)
    let mut click = |label: &str, name: &str, events: &mut Vec<egui::Event>, store: &mut Store, texts: &mut Vec<(String, Rect)>| {
        let at = texts
            .iter()
            .find(|(drawn, _)| drawn == label)
            .unwrap_or_else(|| panic!("“{label}” is drawn"))
            .1
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        for step in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
            *events = step;
            draw(name, 1, events, store, texts);
        }
        draw(name, 3, events, store, texts);
    };
    click(tr!("common-cancel"), "switcher-workspace", &mut events, &mut store, &mut texts);
    click("Sorting", "switcher-workspace", &mut events, &mut store, &mut texts);
    let sorting = store.find_workspace("Sorting").expect("added").1.clone();
    for expected in [
        crate::folderstorm::workspaces::words::workspace_holds(&sorting),
        tr!("fs-workspaces-closes").to_owned(),
    ] {
        assert!(shown(&texts, &expected), "“{expected}” is drawn");
    }
    click(
        tr!("fs-switcher-save-new"),
        "switcher-save-workspace",
        &mut events,
        &mut store,
        &mut texts,
    );
    for expected in [
        tr!("fs-workspaces-save-layout").to_owned(),
        tr!("fs-workspaces-save-workspace").to_owned(),
        tr!("fs-workspaces-with-layout", name = "1 monitor"),
        tr!("fs-workspaces-save-help").to_owned(),
    ] {
        assert!(shown(&texts, &expected), "“{expected}” is drawn");
    }
}
