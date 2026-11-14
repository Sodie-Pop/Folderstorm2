//! Checkpoint 11 drawn by the processor's painter, without a window:
//! "New IM" and "Friends" in the shared chat and in a monitor's chat, what
//! a click there asks of the character's viewer and what its answer
//! shows, a line's links, who types and the nearby chat's volume.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps each picture, as
//! `<folder>/<name>.png`, to be looked at.

use super::super::chat_ui::tests::{heard, host};
use super::monitor_windows::{Act as MonitorAct, Seat};
use super::panels::Which;
use super::tests::window;
use super::*;
use ::folderstorm_session::chat::Viewing;
use ::folderstorm_session::contacts::Which as Sheet;
use ::folderstorm_session::ipc::{
    ChatBatch, Contact, Contacts, Conversation, LineKind, Open, OpenOutcome, OpenRefusal, Screen, SessionKind, Status, ToViewer, Volume,
};
use std::sync::Arc;

const MIA: Conversation = Conversation::Session(0, 3);
const BOB: Conversation = Conversation::Session(0, 4);
const BUILDERS: Conversation = Conversation::Session(0, 11);

/// One text the frame drew: what it says, where its galley is.
struct Drawn {
    text: String,
    at: egui::Pos2,
    galley: Arc<egui::Galley>,
}

/// What a frame drew, the pixels, what it asked of the window and the
/// browser addresses it asked the system to open.
struct Frame {
    texts: Vec<Drawn>,
    pixels: Vec<u32>,
    acts: Vec<Act>,
    opened_urls: Vec<String>,
}

impl Frame {
    fn shows(&self, text: &str) -> bool {
        self.texts.iter().any(|t| t.text.contains(text))
    }

    /// The point of the `nth` character of `needle` in the first text that
    /// holds it, a little inside it.
    fn point_of(&self, needle: &str, nth: usize) -> egui::Pos2 {
        let drawn = self
            .texts
            .iter()
            .find(|t| t.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not drawn"));
        let byte = drawn.text.find(needle).unwrap_or(0);
        let index = drawn.text[..byte].chars().count() + nth;
        let rect = drawn.galley.pos_from_cursor(egui::text::CCursor::new(index));
        drawn.at + rect.center().to_vec2() + egui::vec2(2.0, 0.0)
    }
}

/// One frame of a host's window with what the user does in it.
fn frame(app: &mut Window_, which: Which, (width, height): (u32, u32), events: Vec<egui::Event>) -> Frame {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width as f32, height as f32))),
        events,
        focused: true,
        ..Default::default()
    };
    let view = app.view_mut(which).expect("the window's view");
    let context = view.context.clone();
    let mut painter = std::mem::take(&mut view.painter);
    app.drawing = which;
    let mut acts = Vec::new();
    let mut output = context.run_ui(raw, |ui| acts = app.content(ui));
    app.drawing = Which::Main;
    fn collect(shape: &egui::Shape, into: &mut Vec<Drawn>) {
        match shape {
            egui::Shape::Text(text) => into.push(Drawn {
                text: text.galley.text().to_owned(),
                at: text.pos,
                galley: text.galley.clone(),
            }),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, into)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut texts);
    }
    let opened_urls = output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect();
    let primitives = context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
    let mut pixels = vec![0u32; (width * height) as usize];
    painter.paint(
        &mut pixels,
        [width as usize, height as usize],
        &primitives,
        &output.textures_delta,
        output.pixels_per_point,
        app.palette.panel,
    );
    output.textures_delta.clear();
    if let Some(view) = app.view_mut(which) {
        view.painter = painter;
    }
    Frame {
        texts,
        pixels,
        acts,
        opened_urls,
    }
}

/// A click at `at`: pressed in one frame, released in the next (what the
/// release asks for is the second frame's).
fn click(app: &mut Window_, which: Which, size: (u32, u32), at: egui::Pos2) -> Frame {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(app, which, size, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(app, which, size, vec![press(false)])
}

fn keep(name: &str, frame: &Frame, (width, height): (u32, u32)) {
    let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
        return;
    };
    let rgba: Vec<_> = frame
        .pixels
        .iter()
        .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
        .collect();
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
}

fn contact(n: u128, name: &str, online: bool) -> Contact {
    Contact {
        who: Conversation::session(n),
        name: name.to_owned(),
        online,
    }
}

/// Two characters in the world, launch 1, login 1, the first in front;
/// their viewers told their friends and groups.
fn app_with_contacts() -> Window_ {
    let now = Instant::now();
    let mut app = window(host());
    for slot in 0..2 {
        app.host.desk.launch(slot, slot == 0);
        app.host.desk.connected(slot, 1, now);
        let status = Status {
            screen: Screen::World,
            name: ["Jane Doe", "Sam Resident"][slot].to_owned(),
            ..Default::default()
        };
        app.host.desk.status(slot, 1, status, now);
        app.host.desk.heartbeat(slot, 1, 0, now);
    }
    app.host.desk.take_effects();
    let list = Contacts {
        generation: 1,
        friends: vec![
            contact(1, "Zoe Resident", false),
            contact(2, "Adam Resident", false),
            contact(3, "Mia Resident", true),
            contact(4, "bob Resident", true),
        ],
        groups: vec![contact(10, "Plaza Market", false), contact(11, "Aurora Builders", false)],
        more_friends: 3,
        more_groups: 0,
    };
    for slot in 0..2 {
        let login = app.host.chat.login(slot);
        app.host.book.set(slot, 1, login, list.clone());
    }
    app
}

/// The requests to open written since the last call: (slot, window, what).
fn asked(app: &mut Window_) -> Vec<(usize, u64, Open)> {
    app.host
        .book
        .take_effects()
        .into_iter()
        .filter_map(|send| match send.message {
            ToViewer::Open { open, .. } => Some((send.slot, send.id, open)),
            _ => None,
        })
        .collect()
}

#[test]
fn new_im_lists_the_character_s_groups_and_friends_and_opens_what_its_viewer_opened() {
    let mut app = app_with_contacts();
    let size = (1100, 720);
    app.chat.show_sheet(0, Some(Sheet::Everyone));
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    keep("contacts-new-im-1100", &drawn, size);
    for text in [
        "New IM as Jane Doe",
        "Groups (2)",
        "Aurora Builders",
        "Plaza Market",
        "Friends (4)",
        "Mia Resident",
    ] {
        assert!(
            drawn.shows(text),
            "{text}: {:?}",
            drawn.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
        );
    }
    assert!(drawn.shows("3 more are only in Jane Doe's own lists"));
    // nothing asked by looking
    assert!(asked(&mut app).is_empty());
    // a click on a row asks the character's viewer to open it, nothing more
    let at = drawn.point_of("Mia Resident", 1);
    click(&mut app, Which::Main, size, at);
    let requests = asked(&mut app);
    assert_eq!(requests.len(), 1, "{requests:?}");
    let (slot, id, open) = requests[0];
    assert_eq!((slot, open), (0, Open::Im(MIA)));
    assert!(app.host.chat.take_effects().is_empty(), "nothing said to anyone");
    assert!(app.host.chat.thread(0, MIA).is_none(), "shown once the viewer opened it");
    // the viewer opened it: the tab shows it, ready to type in
    app.host.book.answer(0, 1, id, OpenOutcome::Opened, &mut app.host.chat);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(
        drawn.shows("Send as Jane Doe — Mia Resident"),
        "{:?}",
        drawn.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    assert!(!drawn.shows("New IM as Jane Doe"), "the sheet gave the lines' place back");
    assert_eq!(app.chat.viewing(&app.host.chat), Viewing::Thread(0, MIA));
    keep("contacts-opened-1100", &drawn, size);
    // a search narrows the list; a group's chat opens the same way
    app.chat.show_sheet(0, Some(Sheet::Everyone));
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    let at = drawn.point_of("Aurora Builders", 2);
    click(&mut app, Which::Main, size, at);
    let requests = asked(&mut app);
    assert_eq!(requests.iter().map(|r| r.2).collect::<Vec<_>>(), [Open::Group(BUILDERS)]);
    app.host.book.answer(0, 1, requests[0].1, OpenOutcome::Opened, &mut app.host.chat);
    frame(&mut app, Which::Main, size, Vec::new());
    assert_eq!(app.host.chat.thread(0, BUILDERS).map(|t| t.kind), Some(SessionKind::Group));
}

#[test]
fn friends_are_online_first_and_a_refusal_is_said_where_it_was_asked() {
    let mut app = app_with_contacts();
    let size = (1100, 720);
    app.chat.show_sheet(0, Some(Sheet::Friends));
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    for text in ["Jane Doe's friends", "Online (2)", "Offline (2)", "bob Resident", "Zoe Resident"] {
        assert!(drawn.shows(text), "{text}");
    }
    assert!(!drawn.shows("Aurora Builders"), "no group among the friends");
    // online first: bob and Mia above Adam and Zoe
    let y = |name: &str| drawn.point_of(name, 0).y;
    assert!(y("bob Resident") < y("Mia Resident") && y("Mia Resident") < y("Adam Resident") && y("Adam Resident") < y("Zoe Resident"));
    keep("contacts-friends-1100", &drawn, size);
    // narrow: the same, in the chat's page
    app.chat_page = true;
    let narrow = (480, 640);
    let drawn = frame(&mut app, Which::Main, narrow, Vec::new());
    assert!(drawn.shows("bob Resident") && drawn.shows("Online (2)"));
    keep("contacts-friends-480", &drawn, narrow);
    app.chat_page = false;
    // the viewer refuses (RLVa): a notice where it was asked, nothing opened
    let now = Instant::now();
    let login = app.host.chat.login(0);
    let id = app
        .host
        .book
        .open(
            0,
            login,
            ::folderstorm_session::chat::SHARED_WINDOW,
            Open::Im(BOB),
            "bob Resident",
            now,
        )
        .expect("asked");
    app.host
        .book
        .answer(0, 1, id, OpenOutcome::Refused(OpenRefusal::Restricted), &mut app.host.chat);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(
        drawn.shows("Jane Doe did not open bob Resident: a restriction forbids it."),
        "{:?}",
        drawn.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    assert!(app.host.chat.thread(0, BOB).is_none());
    keep("contacts-refused-1100", &drawn, size);
    // a character not in the world: its sheet says so, and asks nothing
    app.chat.show_sheet(1, Some(Sheet::Friends));
    app.host.chat.ended(1, 1);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(drawn.shows("Sam Resident is not in the world."));
}

#[test]
fn a_monitor_s_chat_has_new_im_and_shows_what_its_viewer_opened() {
    let now = Instant::now();
    let mut app = app_with_contacts();
    let seat = Seat::Slot(1);
    app.host.desk.open_monitor(1);
    let view = app.look.view();
    app.monitors.test_open(seat, view);
    app.apply(vec![Act::Monitors(MonitorAct::Chat(seat, true))], now);
    heard(
        &mut app.host,
        1,
        Conversation::Nearby,
        1,
        LineKind::Said,
        "Jane Doe",
        "hello neighbours",
    );
    let (which, size) = (Which::Monitor(1), (480, 490));
    app.monitors.test_sheet(seat, 1, Some(Sheet::Everyone));
    let drawn = frame(&mut app, which, size, Vec::new());
    keep("contacts-monitor-new-im-480", &drawn, size);
    assert!(
        drawn.shows("Aurora Builders") && drawn.shows("Mia Resident"),
        "{:?}",
        drawn.texts.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    let at = drawn.point_of("Plaza Market", 1);
    click(&mut app, which, size, at);
    let requests = asked(&mut app);
    assert_eq!(
        requests.iter().map(|r| r.2).collect::<Vec<_>>(),
        [Open::Group(Conversation::session(10))]
    );
    app.host.book.answer(1, 1, requests[0].1, OpenOutcome::Opened, &mut app.host.chat);
    let drawn = frame(&mut app, which, size, Vec::new());
    assert_eq!(
        app.monitors.test_shown(seat, 1),
        (Conversation::session(10), false),
        "shown in the monitor that asked"
    );
    assert_eq!(app.chat.viewing(&app.host.chat), Viewing::All, "not in the shared chat");
    assert!(drawn.shows("Plaza Market"));
    keep("contacts-monitor-opened-480", &drawn, size);
    // the Friends sheet at the least size
    app.monitors.test_sheet(seat, 1, Some(Sheet::Friends));
    let drawn = frame(&mut app, which, (320, 400), Vec::new());
    assert!(drawn.shows("bob Resident"));
    keep("contacts-monitor-friends-320", &drawn, (320, 400));
}

/// The link's question and both its buttons drawn inside a window of
/// that size.
fn question_inside(drawn: &Frame, (width, height): (u32, u32), name: &str) {
    for text in ["Open this address in your browser?", "Open in browser", "Cancel"] {
        let at = drawn.point_of(text, 1);
        assert!(
            at.x > 0.0 && at.x < width as f32 && at.y > 30.0 && at.y < height as f32 - 4.0,
            "{name}: {text:?} at {at:?}"
        );
    }
}

#[test]
fn a_link_s_question_in_a_monitor_s_chat_is_never_cut_off() {
    let now = Instant::now();
    let mut app = app_with_contacts();
    let seat = Seat::Slot(1);
    app.host.desk.open_monitor(1);
    let view = app.look.view();
    app.monitors.test_open(seat, view);
    app.apply(vec![Act::Monitors(MonitorAct::Chat(seat, true))], now);
    for number in 1..=8 {
        heard(
            &mut app.host,
            1,
            Conversation::Nearby,
            number,
            LineKind::Said,
            "Jane Doe",
            &format!("line {number}: see https://example.org/page{number}"),
        );
    }
    let which = Which::Monitor(1);
    // the human's window, the first size, and smaller ones
    for size in [(480, 740), (320, 400), (260, 300), (220, 230)] {
        let name = format!("monitor-link-{}x{}", size.0, size.1);
        // (a new size: the lines settle at their bottom in a frame)
        frame(&mut app, which, size, Vec::new());
        let drawn = frame(&mut app, which, size, Vec::new());
        let at = drawn.point_of("example.org/page8", 3);
        let clicked = click(&mut app, which, size, at);
        assert!(clicked.opened_urls.is_empty(), "{name}: asked first");
        // (asked in that frame: drawn whole from the next one)
        let drawn = frame(&mut app, which, size, Vec::new());
        keep(&name, &drawn, size);
        question_inside(&drawn, size, &name);
        // its button opens the page, once, and the question goes
        let at = drawn.point_of("Open in browser", 2);
        let clicked = click(&mut app, which, size, at);
        assert_eq!(clicked.opened_urls, ["https://example.org/page8"], "{name}");
        let drawn = frame(&mut app, which, size, Vec::new());
        assert!(!drawn.shows("Open this address in your browser?"), "{name}");
    }
}

#[test]
fn a_link_s_question_in_a_small_shared_chat_keeps_its_buttons() {
    let mut app = app_with_contacts();
    heard(
        &mut app.host,
        0,
        Conversation::Nearby,
        1,
        LineKind::Said,
        "Jane Doe",
        "see https://example.org/page",
    );
    app.chat.show_sheet(0, None);
    app.host.desk.set_panel_out(Panel::Chat, true);
    app.panes[Panel::Chat.index()] = Some(app.look.view());
    let which = Which::Pane(Panel::Chat);
    let size = (420, 420);
    let drawn = frame(&mut app, which, size, Vec::new());
    let at = drawn.point_of("example.org", 2);
    click(&mut app, which, size, at);
    assert!(app.chat.test_link().is_some());
    let drawn = frame(&mut app, which, size, Vec::new());
    keep("chat-link-question-420", &drawn, size);
    question_inside(&drawn, size, "shared chat");
}

#[test]
fn a_web_link_opens_after_the_viewer_s_judge_and_a_grid_link_switches() {
    let mut app = app_with_contacts();
    let size = (1100, 720);
    heard(
        &mut app.host,
        0,
        Conversation::Nearby,
        1,
        LineKind::Said,
        "Jane Doe",
        "see https://example.org/page or secondlife://Ahern/128/128/30 and https://secondlife.com/destinations",
    );
    app.chat.show_sheet(0, None);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    keep("chat-links-1100", &drawn, size);
    // an unknown site: asked first, nothing opened
    let at = drawn.point_of("example.org", 2);
    let clicked = click(&mut app, Which::Main, size, at);
    assert!(clicked.opened_urls.is_empty());
    assert_eq!(
        app.chat.test_link().map(|ask| ask.target.as_str()),
        Some("https://example.org/page")
    );
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(drawn.shows("Open this address in your browser?"));
    keep("chat-link-question-1100", &drawn, size);
    // "Open in browser" opens it, once
    let at = drawn.point_of("Open in browser", 1);
    let clicked = click(&mut app, Which::Main, size, at);
    assert_eq!(clicked.opened_urls, ["https://example.org/page"]);
    assert!(app.chat.test_link().is_none());
    // a trusted site opens at once
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    let at = drawn.point_of("secondlife.com/destinations", 3);
    let clicked = click(&mut app, Which::Main, size, at);
    assert_eq!(clicked.opened_urls, ["https://secondlife.com/destinations"]);
    assert!(app.chat.test_link().is_none());
    // a grid's link is never followed here: the character comes forward
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    let at = drawn.point_of("secondlife://Ahern", 4);
    let clicked = click(&mut app, Which::Main, size, at);
    assert!(clicked.opened_urls.is_empty());
    assert!(
        clicked.acts.iter().any(|act| matches!(act, Act::Switch(0))),
        "switch to the character"
    );
    // a click elsewhere on the line opens nothing
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    let at = drawn.point_of("see ", 1);
    let clicked = click(&mut app, Which::Main, size, at);
    assert!(clicked.opened_urls.is_empty() && app.chat.test_link().is_none());
    // what is not a web page never reaches the browser, nor the question
    let ctx = egui::Context::default();
    assert!(super::super::chat_ui::follow_web(&ctx, "file:///C:/Windows/notepad.exe").is_none());
    assert!(super::super::chat_ui::follow_web(&ctx, "https://127.0.0.1/admin").is_some_and(|ask| ask.why.is_some()));
}

#[test]
fn who_types_shows_above_the_box_and_ctrl_enter_shouts() {
    let mut app = app_with_contacts();
    let size = (1100, 720);
    heard(&mut app.host, 0, Conversation::Nearby, 1, LineKind::Said, "Jane Doe", "hello");
    app.host.chat.batch(
        0,
        1,
        ChatBatch {
            generation: 1,
            conversation: MIA,
            kind: SessionKind::Resident,
            title: "Mia Resident".into(),
            typing: true,
            ..Default::default()
        },
    );
    app.chat.opened(0, MIA);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(drawn.shows("Mia Resident is typing…"));
    keep("chat-typing-1100", &drawn, size);
    // the nearby chat: Ctrl+Enter shouts a line, whatever is chosen
    app.chat.opened(0, Conversation::Nearby);
    let drawn = frame(&mut app, Which::Main, size, Vec::new());
    assert!(!drawn.shows("is typing"));
    let at = drawn.point_of("Type as Jane Doe", 2);
    click(&mut app, Which::Main, size, at);
    frame(&mut app, Which::Main, size, vec![egui::Event::Text("over here".into())]);
    assert_eq!(app.host.chat.draft(0, Conversation::Nearby), "over here");
    app.host.chat.take_effects();
    let key = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    frame(&mut app, Which::Main, size, vec![key(true)]);
    frame(&mut app, Which::Main, size, vec![key(false)]);
    let volumes: Vec<Volume> = app
        .host
        .chat
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            ::folderstorm_session::chat::Effect::Send {
                message: ToViewer::SendChat { volume, text, .. },
                ..
            } if text == "over here" => Some(volume),
            _ => None,
        })
        .collect();
    assert_eq!(volumes, [Volume::Shout]);
    assert_eq!(app.host.chat.volume(0), Volume::Normal, "the choice stays as it was");
}
