//! The confirmation before a new viewer, in the host's own window: the
//! row of quick actions, the characters' cards and the condensed lines (the
//! bar's own, docked and in the compact window, are tested with its
//! drawing, `folderstorm_characters/bar.rs`), and the way from an empty line
//! of the bar's list to the new profile page. Drawn by the processor's
//! painter without a window; `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps
//! each picture, as `<folder>/<name>.png`.

use super::super::chat_ui::tests::host;
use super::comfort::{Act as ComfortAct, Page};
use super::panels::Which;
use super::tests::window;
use super::*;
use ::folderstorm_session::bar::Request;

const SIZE: (u32, u32) = (1120, 760);

struct Frame {
    texts: Vec<(String, egui::Pos2)>,
    pixels: Vec<u32>,
    acts: Vec<Act>,
}

impl Frame {
    fn shows(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _)| t == text)
    }

    /// A point inside the button whose text is exactly `text`.
    fn at(&self, text: &str) -> egui::Pos2 {
        let (_, pos) = self
            .texts
            .iter()
            .find(|(t, _)| t == text)
            .unwrap_or_else(|| panic!("{text:?} is not drawn: {:?}", self.texts));
        *pos + egui::vec2(3.0, 5.0)
    }

    /// Whether the frame asked for this character's quick action, from the
    /// row or a line (`Quick`), or from its card (`Switch`).
    fn asked_quick(&self, slot: usize) -> bool {
        self.acts
            .iter()
            .any(|act| matches!(act, Act::Comfort(ComfortAct::Quick(s)) | Act::Switch(s) if *s == slot))
    }
}

/// One frame of the host's main window at `time` seconds.
fn frame(app: &mut Window_, time: f64, events: Vec<egui::Event>) -> Frame {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(SIZE.0 as f32, SIZE.1 as f32))),
        time: Some(time),
        events,
        focused: true,
        ..Default::default()
    };
    let which = Which::Main;
    let view = app.view_mut(which).expect("the window's view");
    let context = view.context.clone();
    let mut painter = std::mem::take(&mut view.painter);
    app.drawing = which;
    let mut acts = Vec::new();
    let mut output = context.run_ui(raw, |ui| acts = app.content(ui));
    app.drawing = Which::Main;
    fn collect(shape: &egui::Shape, into: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(text) => into.push((text.galley.text().to_owned(), text.pos)),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, into)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut texts);
    }
    let primitives = context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
    let mut pixels = vec![0u32; (SIZE.0 * SIZE.1) as usize];
    painter.paint(
        &mut pixels,
        [SIZE.0 as usize, SIZE.1 as usize],
        &primitives,
        &output.textures_delta,
        output.pixels_per_point,
        app.palette.panel,
    );
    output.textures_delta.clear();
    if let Some(view) = app.view_mut(which) {
        view.painter = painter;
    }
    Frame { texts, pixels, acts }
}

/// A click: the pointer comes, then presses and lets go. What the click
/// asks of the window is the last frame's.
fn click(app: &mut Window_, time: f64, at: egui::Pos2) -> Frame {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(app, time - 0.02, vec![egui::Event::PointerMoved(at)]);
    frame(app, time, vec![press(true), press(false)])
}

fn keep(name: &str, frame: &Frame) {
    let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
        return;
    };
    let rgba: Vec<_> = frame
        .pixels
        .iter()
        .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
        .collect();
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    image::save_buffer(path, &rgba, SIZE.0, SIZE.1, image::ColorType::Rgba8).expect("capture written");
}

/// The second character in the world, the first one without a viewer.
fn second_runs(app: &mut Window_, now: Instant) {
    app.host.desk.launch(1, true);
    app.host.desk.connected(1, 1, now);
    app.host.desk.take_effects();
}

#[test]
fn a_quick_action_of_the_row_asks_launch_for_a_character_without_a_viewer() {
    let now = Instant::now();
    let mut app = window(host());
    second_runs(&mut app, now);
    frame(&mut app, 1.0, vec![]);
    let idle = frame(&mut app, 1.1, vec![]);
    assert!(idle.shows("1 · demo-1") && idle.shows("2 · demo-2") && !idle.shows("Launch?"));
    // the first click only asks
    let first = click(&mut app, 2.0, idle.at("1 · demo-1"));
    assert!(!first.asked_quick(0), "nothing starts at the first click");
    let armed = frame(&mut app, 2.1, vec![]);
    assert!(armed.shows(tr!("folderstorm-quick-confirm")) && !armed.shows("1 · demo-1"));
    keep("host-row-launch-armed", &armed);
    // the second one, in time, starts it
    let second = click(&mut app, 4.0, armed.at("Launch?"));
    assert!(second.asked_quick(0));
    let after = frame(&mut app, 4.1, vec![]);
    assert!(after.shows("1 · demo-1") && !after.shows("Launch?"));
    // it reverts when the time runs out
    click(&mut app, 10.0, after.at("1 · demo-1"));
    assert!(frame(&mut app, 10.5, vec![]).shows("Launch?"));
    assert!(!frame(&mut app, 14.5, vec![]).shows("Launch?"), "four seconds");
    // a character in the world is one click
    let one = click(&mut app, 20.0, after.at("2 · demo-2"));
    assert!(one.asked_quick(1));
}

#[test]
fn a_card_asks_launch_before_it_starts_a_viewer() {
    let now = Instant::now();
    let mut app = window(host());
    second_runs(&mut app, now);
    frame(&mut app, 1.0, vec![]);
    let idle = frame(&mut app, 1.1, vec![]);
    let launch = tr!("folderstorm-host-launch");
    assert!(idle.shows(launch), "{:?}", idle.texts);
    let first = click(&mut app, 2.0, idle.at(launch));
    assert!(
        !first.acts.iter().any(|act| matches!(act, Act::Switch(_))),
        "the first click only asks"
    );
    let armed = frame(&mut app, 2.1, vec![]);
    assert!(armed.shows("Launch?"), "{:?}", armed.texts);
    assert!(!armed.shows(launch), "the card's button says Launch?");
    keep("host-card-launch-armed", &armed);
    let second = click(&mut app, 3.5, armed.at("Launch?"));
    assert!(second.acts.iter().any(|act| matches!(act, Act::Switch(0))));
    // the pointer leaves: it reverts
    let idle = frame(&mut app, 4.0, vec![]);
    click(&mut app, 6.0, idle.at(launch));
    assert!(frame(&mut app, 6.1, vec![]).shows("Launch?"));
    frame(&mut app, 6.5, vec![egui::Event::PointerMoved(egui::pos2(1000.0, 700.0))]);
    assert!(!frame(&mut app, 6.6, vec![]).shows("Launch?"));
    // the switch of a character in the world is one click
    assert!(idle.shows(tr!("folderstorm-host-show")) || idle.shows(tr!("folderstorm-host-switch")));
}

#[test]
fn a_condensed_line_asks_launch_before_it_starts_a_viewer() {
    let now = Instant::now();
    let mut app = window(host());
    second_runs(&mut app, now);
    app.host.desk.set_condensed(true);
    frame(&mut app, 1.0, vec![]);
    let idle = frame(&mut app, 1.1, vec![]);
    let launch = tr!("folderstorm-host-launch");
    assert!(idle.shows(launch), "{:?}", idle.texts);
    let first = click(&mut app, 2.0, idle.at(launch));
    assert!(!first.asked_quick(0));
    let armed = frame(&mut app, 2.1, vec![]);
    assert!(armed.shows("Launch?") && !armed.shows(launch), "{:?}", armed.texts);
    keep("host-line-launch-armed", &armed);
    let second = click(&mut app, 3.0, armed.at("Launch?"));
    assert!(second.asked_quick(0));
    // the line of a character in the world: one click
    let idle = frame(&mut app, 3.5, vec![]);
    let switch = tr!("folderstorm-host-show");
    let one = click(&mut app, 5.0, idle.at(switch));
    assert!(one.asked_quick(1));
}

#[test]
fn an_empty_line_of_the_bar_s_list_opens_the_new_profile_page() {
    let now = Instant::now();
    let mut app = window(host());
    // the host's own window is hidden behind a docked bar, say
    app.bar_apply(Request::Show(::folderstorm_session::bar::Page::NewProfile), now);
    assert_eq!(app.comfort.page, Page::NewProfile);
    frame(&mut app, 1.0, vec![]);
    let page = frame(&mut app, 1.1, vec![]);
    assert!(page.shows(tr!("folderstorm-new-profile-heading")), "{:?}", page.texts);
    keep("host-new-profile-from-the-bar", &page);
}
