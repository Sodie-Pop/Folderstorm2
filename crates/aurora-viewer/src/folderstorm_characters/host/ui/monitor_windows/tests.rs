//! The monitors' windows drawn by the processor's painter, without a
//! window: what each shows, what its buttons do, the swap on "Switch",
//! the chips at several widths, and the cycling monitor's turns.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps each picture, as
//! `<folder>/<name>.png`, to be looked at.

use super::super::super::chat_ui::tests::host;
use super::super::tests::{draw, draw_in, draw_with, window};
use super::*;
use ::folderstorm_session::host::Effect;
use ::folderstorm_session::identity::Colour;
use ::folderstorm_session::ipc::{ChatBatch, ChatLine, LineKind, Role, Screen, SessionKind, Status, ToViewer};
use ::folderstorm_session::monitor::State;
use std::time::Duration;

const RED: egui::Color32 = egui::Color32::from_rgb(200, 30, 30);
const GREEN: egui::Color32 = egui::Color32::from_rgb(20, 180, 60);
const JANE: Conversation = Conversation::Session(0, 0x4a);
const NAMES: [&str; 5] = ["Jane Doe", "Sam Resident", "Kim Resident", "Lee Resident", "Max Resident"];

fn shows(drawn: &[String], text: &str) -> bool {
    drawn.iter().any(|t| t.contains(text))
}

fn count(pixels: &[u32], colour: egui::Color32) -> usize {
    let wanted = u32::from_be_bytes([0, colour.r(), colour.g(), colour.b()]);
    pixels.iter().filter(|p| **p == wanted).count()
}

fn keep(name: &str, pixels: &[u32], width: u32, height: u32) {
    let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
        return;
    };
    let rgba: Vec<_> = pixels
        .iter()
        .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
        .collect();
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
}

fn report(app: &mut Window_, slot: usize, monitor: State, now: Instant) {
    let status = Status {
        screen: Screen::World,
        name: NAMES[slot].to_owned(),
        monitor,
        ..Default::default()
    };
    app.host.desk.status(slot, 1, status, now);
    // a status comes over a live link, whose heartbeat comes with it:
    // without it a slow test run passes the silence limit and the
    // monitor says "not answering" instead
    app.host.desk.heartbeat(slot, 1, 0, now);
}

/// `count` characters in the world (the demo's two, and more as asked),
/// the first one in front, each with its name, the first three with a
/// colour and the first two with an alias.
fn running(app: &mut Window_, count: usize, now: Instant) {
    for extra in 2..count {
        app.host.desk.add(&format!("demo-{}", extra + 1)).expect("a character");
    }
    for slot in 0..count {
        app.host.desk.launch(slot, slot == 0);
        app.host.desk.connected(slot, 1, now);
        report(app, slot, State::Off, now);
        app.host.chat.status(slot, 1, 1, true);
    }
    for (slot, colour) in [Colour::Teal, Colour::Orange, Colour::Violet].into_iter().enumerate().take(count) {
        app.host.desk.set_colour(slot, colour);
    }
    for (slot, alias) in ["Main", "Shop keeper"].into_iter().enumerate().take(count) {
        app.host.desk.set_alias(slot, alias).expect("an alias");
    }
    app.host.desk.take_effects();
}

/// Opens a seat's monitor and gives it a view, as `sync_monitors` gives
/// it a window.
fn open(app: &mut Window_, seat: Seat) {
    match seat {
        Seat::Slot(slot) => app.host.desk.open_monitor(slot),
        Seat::Cycling => app.host.desk.set_cycling(true),
    }
    let view = app.look.view();
    app.monitors.test_open(seat, view);
}

fn focus(app: &mut Window_, seat: Seat, focused: bool) {
    if let Some(view) = app.monitors.view_mut(seat) {
        view.focused = focused;
    }
}

fn cover(app: &mut Window_, seat: Seat, covered: bool) {
    if let Some(view) = app.monitors.view_mut(seat) {
        view.occluded = covered;
    }
}

/// What a click asks for is carried out, then the windows follow the
/// desk, as before the next wait (`sync_monitors`).
fn act(app: &mut Window_, act: Act, now: Instant) {
    app.apply(vec![HostAct::Monitors(act)], now);
    app.follow_moves();
}

/// A line of someone else in a conversation of `slot`.
fn said(app: &mut Window_, slot: usize, conversation: Conversation, kind: SessionKind, title: &str, number: u64, text: &str) {
    app.host.chat.batch(
        slot,
        1,
        ChatBatch {
            generation: 1,
            conversation,
            kind,
            title: title.to_owned(),
            lines: vec![ChatLine {
                number,
                time: 0,
                kind: LineKind::Said,
                emote: false,
                from: if title.is_empty() {
                    "Jane Doe".to_owned()
                } else {
                    title.to_owned()
                },
                text: text.to_owned(),
            }],
            ..Default::default()
        },
    );
}

fn nearby(app: &mut Window_, slot: usize, number: u64, text: &str) {
    said(app, slot, Conversation::Nearby, SessionKind::Nearby, "", number, text);
}

/// `count` conversations with residents, each with one line.
fn many(app: &mut Window_, slot: usize, count: u64) {
    let names = [
        "Alexandria Featherstonehaugh",
        "Bo Li",
        "Christopher Longbottom-Smythe",
        "Dee Dee",
        "Evangeline Rosewater",
        "Fabrizio Montague",
        "Gwendolyn Ashworth",
        "Hal Ito",
        "Isabella Montgomery",
        "Jo Park",
        "Katarzyna Wojciechowska",
        "Leo Sun",
    ];
    for n in 0..count {
        let name = names[(n as usize) % names.len()];
        said(
            app,
            slot,
            Conversation::Session(2, 100 + n),
            SessionKind::Resident,
            name,
            1,
            "hello",
        );
    }
}

/// The lines the chat handed to viewers since the last look.
fn sent(app: &mut Window_) -> Vec<(usize, Conversation, String)> {
    app.host
        .chat
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            ::folderstorm_session::chat::Effect::Send {
                slot,
                message: ToViewer::SendChat { conversation, text, .. },
                ..
            } => Some((slot, conversation, text)),
            _ => None,
        })
        .collect()
}

/// Whether the desk told that viewer, since the last look, to be a plain
/// background character.
fn sent_to_the_background(app: &mut Window_, slot: usize) -> bool {
    app.host.desk.take_effects().iter().any(|effect| {
        matches!(
            effect,
            Effect::Send { slot: to, message: ToViewer::Assign(a), .. } if *to == slot && a.role == Role::Background
        )
    })
}

fn texture(app: &Window_, seat: Seat, slot: usize) -> Option<egui::TextureId> {
    app.monitors
        .view(seat)
        .and_then(|view| view.monitors.texture(slot))
        .map(egui::TextureHandle::id)
}

#[test]
fn a_monitor_s_window_shows_the_latest_picture_and_pauses_with_it() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    assert_eq!(app.host.desk.watch(1), Watch::Hidden, "not drawn yet: nothing asked");
    // drawn, then seen, then asked of its viewer
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert!(app.monitors.view(seat).is_some_and(|view| view.monitors.seen[1]));
    app.monitors_watch(now);
    assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
    // the header names the character (its alias); nothing says "read-only"
    assert!(shows(&drawn, "Shop keeper"), "{drawn:?}");
    assert!(!drawn.iter().any(|t| t.to_lowercase().contains("read-only")), "{drawn:?}");
    // (reported now: the drawings above can take seconds on a slow
    // machine, and a status that old would read as a viewer not answering)
    report(&mut app, 1, State::Off, Instant::now());
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert!(shows(&drawn, tr!("folderstorm-monitor-waiting")), "{drawn:?}");
    keep("monitor-480-waiting", &pixels, 480, 300);
    // a picture comes: painted whole under the header, in the window that
    // shows that character and in no other
    app.host.test_picture(1, [480, 270], RED, now);
    report(&mut app, 1, State::Live, now);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert_eq!(count(&pixels, RED), 480 * 270, "the picture, 1:1");
    assert!(!shows(&drawn, tr!("folderstorm-monitor-waiting")));
    keep("monitor-480", &pixels, 480, 300);
    let orange = faces::tint(&app.palette, Colour::Orange).expect("a colour");
    assert!(count(&pixels, orange) > 20, "the character's colour is in the header");
    let (_, pixels) = draw(&mut app, 1120, 760);
    assert_eq!(count(&pixels, RED), 0, "the host's own window draws no picture");
    // the next one replaces it in the same texture
    let id = texture(&app, seat, 1);
    assert!(id.is_some());
    app.host.test_picture(1, [480, 270], GREEN, now);
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert_eq!(texture(&app, seat, 1), id);
    assert_eq!((count(&pixels, RED), count(&pixels, GREEN)), (0, 480 * 270));
    // how it does is said in the hover
    report(&mut app, 1, State::Slowed, now);
    let state = app.host.desk.slot(1).expect("slot").clone();
    let (status, notice) = app.monitor_status(seat, 1, &state, now);
    assert!(status.contains(tr!("folderstorm-monitor-slowed")) && notice, "{status}");
    // its window is covered or minimised: its viewer is a plain background
    // character again
    app.host.desk.take_effects();
    cover(&mut app, seat, true);
    app.monitors_watch(now);
    assert_eq!(app.host.desk.watch(1), Watch::Hidden);
    assert!(sent_to_the_background(&mut app, 1));
    // the host's own window covered or not changes nothing for it
    app.main.occluded = true;
    cover(&mut app, seat, false);
    app.monitors_watch(now);
    assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
    // closed: the picture is dropped at once
    app.host.desk.set_monitor(1, None);
    app.host.tick_monitors(now);
    assert!(app.host.screens[1].shown.is_none() && app.host.screens[1].picture.is_none());
}

#[test]
fn a_picture_keeps_its_proportions_in_a_window_of_any_size() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    open(&mut app, Seat::Slot(1));
    // a 4:3 picture in a 16:9 room: bars left and right, never stretched
    app.host.test_picture(1, [360, 270], RED, now);
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert_eq!(count(&pixels, RED), 360 * 270);
    // a small window: scaled down whole
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 240, 165);
    let red = count(&pixels, RED);
    assert!((180 * 135 - 700..=180 * 135 + 700).contains(&red), "{red}");
    keep("monitor-240", &pixels, 240, 165);
    // the smallest window: header and picture, nothing over anything
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 220, 120);
    assert!(!shows(&drawn, "use of widget"), "{drawn:?}");
    assert!(count(&pixels, RED) > 0);
    keep("monitor-220", &pixels, 220, 120);
}

#[test]
fn switch_swaps_the_window_to_the_character_that_was_in_front() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 3, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    act(&mut app, Act::Pin(seat, true), now);
    act(&mut app, Act::Chat(seat, true), now);
    app.host.desk.edit_monitor_window(1, |w| {
        w.placed(Place {
            position: Some([900, 40]),
            size: [480, 490],
        });
    });
    draw_in(&mut app, Which::Monitor(1), 480, 490);
    app.host.test_picture(1, [480, 270], RED, now);
    // the window's Switch: carried out in that window's own frame, its
    // view stays where it is until the windows are waited on
    app.apply(vec![HostAct::Monitors(Act::Switch(seat, 1))], now);
    assert_eq!(app.host.desk.active(), Some(1));
    assert!(app.monitors.view(Seat::Slot(1)).is_some(), "not moved under its own drawing");
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert!(!shows(&drawn, "use of widget"), "a frame in between draws: {drawn:?}");
    app.follow_moves();
    // the same window (its view) is now the monitor of the one that left
    assert!(app.monitors.view(Seat::Slot(1)).is_none());
    assert!(app.monitors.view(Seat::Slot(0)).is_some());
    let moved = app.host.desk.slot(0).expect("slot");
    assert!(moved.monitor.is_some());
    assert!(moved.monitor_window.pinned && moved.monitor_window.chat);
    assert_eq!(moved.monitor_window.place.map(|place| place.position), Some(Some([900, 40])));
    assert!(app.host.desk.slot(1).is_some_and(|s| s.monitor.is_none()));
    // it goes on asking, without a look in between that asks nothing:
    // the window is on screen as it was
    app.host.desk.take_effects();
    app.monitors_watch(now);
    assert!(matches!(app.host.desk.watch(0), Watch::Asked { .. }));
    assert!(!sent_to_the_background(&mut app, 0));
    // it shows who it shows now
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(0), 480, 490);
    assert!(shows(&drawn, "Main") && !shows(&drawn, "Shop keeper"), "{drawn:?}");
    assert_eq!(count(&pixels, RED), 0, "nothing of the other character's picture");
    app.monitors_watch(now);
    assert!(matches!(app.host.desk.watch(0), Watch::Asked { .. }));
    assert_eq!(app.host.desk.watch(1), Watch::Closed);
    // a hotkey does the same as the button: back again
    app.host.desk.switch(0, now);
    app.follow_moves();
    assert!(app.monitors.view(Seat::Slot(1)).is_some() && app.monitors.view(Seat::Slot(0)).is_none());
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert!(shows(&drawn, "Shop keeper"), "{drawn:?}");
    // a character without a monitor comes to the front: the window stays
    app.host.desk.switch(2, now);
    app.follow_moves();
    assert!(app.monitors.view(Seat::Slot(1)).is_some());
    assert!(app.host.desk.slot(0).is_some_and(|s| s.monitor.is_none()));
}

#[test]
fn the_header_s_buttons_pin_show_the_chat_and_open_the_options() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    app.host.test_picture(1, [480, 270], RED, now);
    app.host.desk.take_effects();
    // pinned: saved, a filled pin on the accent; no viewer is told
    act(&mut app, Act::Pin(seat, true), now);
    assert!(app.host.desk.slot(1).is_some_and(|s| s.monitor_window.pinned));
    let effects = app.host.desk.take_effects();
    assert!(effects.iter().all(|e| matches!(e, Effect::Save)), "{effects:?}");
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert!(count(&pixels, app.palette.violet) > 100, "the pin is on the accent");
    keep("monitor-480-pinned", &pixels, 480, 300);
    act(&mut app, Act::Pin(seat, false), now);
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert_eq!(count(&pixels, app.palette.violet), 0);
    // the options, inside the window: sizes, rates, the budget's note,
    // "Show chat", the defaults
    act(&mut app, Act::Options(seat, true), now);
    draw_in(&mut app, Which::Monitor(1), 480, 300);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    for text in [
        "320 × 180",
        "480 × 270",
        "640 × 360",
        tr!("folderstorm-monitor-budget-note"),
        tr!("folderstorm-monitor-show-chat"),
    ] {
        assert!(shows(&drawn, text), "{text}: {drawn:?}");
    }
    for rate in ["0.5", "1", "2", "5"] {
        assert!(shows(&drawn, &tr!("folderstorm-monitor-rate", rate = rate)), "{rate}");
    }
    assert!(
        !shows(&drawn, tr!("folderstorm-cycling-pause-typing")),
        "the cycling monitor's alone"
    );
    assert!(!shows(&drawn, "use of widget"), "two widgets share an id: {drawn:?}");
    assert_eq!(count(&pixels, RED), 0, "the options are in the picture's place");
    keep("monitor-480-options", &pixels, 480, 300);
    // while they cover the picture, nothing is asked of the viewer
    app.monitors_watch(now);
    assert_eq!(app.host.desk.watch(1), Watch::Hidden);
    // they fit a small window by scrolling
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 320, 210);
    assert!(shows(&drawn, "320 × 180") && !shows(&drawn, "use of widget"), "{drawn:?}");
    keep("monitor-320-options", &pixels, 320, 210);
    // a tall one shows them all, the defaults' button too
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert!(shows(&drawn, tr!("folderstorm-monitor-use-as-defaults")), "{drawn:?}");
    // a size and a rate: this monitor's own
    let small = Spec {
        size: Size::Small,
        rate: Rate::Five,
    };
    act(&mut app, Act::Spec(seat, small), now);
    assert_eq!(app.host.desk.slot(1).and_then(|s| s.monitor), Some(small));
    assert_eq!(app.host.desk.monitors().defaults, Spec::default());
    // "Use this size and rate as monitor defaults"
    act(&mut app, Act::Defaults(small), now);
    assert_eq!(app.host.desk.monitors().defaults, small);
    app.host.desk.open_monitor(0);
    assert_eq!(app.host.desk.slot(0).and_then(|s| s.monitor), Some(small));
    // "Show chat" is the Chat button
    act(&mut app, Act::Chat(seat, true), now);
    assert!(app.host.desk.slot(1).is_some_and(|s| s.monitor_window.chat));
    act(&mut app, Act::Options(seat, false), now);
    let (_, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert!(count(&pixels, RED) > 0, "the picture is back");
}

#[test]
fn the_chat_part_shows_the_character_s_chips_and_lines_and_reads_them_with_the_keyboard() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    act(&mut app, Act::Chat(seat, true), now);
    nearby(&mut app, 1, 1, "hello neighbours");
    said(&mut app, 1, JANE, SessionKind::Resident, "Jane Doe", 1, "a private word");
    // (the other character's lines are not this window's)
    nearby(&mut app, 0, 1, "somewhere else entirely");
    app.host.test_picture(1, [480, 270], RED, now);
    assert_eq!(app.host.chat.unread(1), 2);
    // without the keyboard: shown, not read
    draw_in(&mut app, Which::Monitor(1), 480, 490);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert!(shows(&drawn, "hello neighbours") && !shows(&drawn, "somewhere else"), "{drawn:?}");
    assert!(shows(&drawn, tr!("common-nearby-chat")) && shows(&drawn, "Jane Doe"), "{drawn:?}");
    assert!(!shows(&drawn, "Send as"), "the header says who speaks: {drawn:?}");
    assert_eq!(count(&pixels, RED), 480 * 270, "the picture keeps its room");
    assert_eq!(app.host.chat.unread(1), 2);
    keep("monitor-480-chat", &pixels, 480, 490);
    // with the keyboard: the conversation on screen is read, the other not
    focus(&mut app, seat, true);
    draw_in(&mut app, Which::Monitor(1), 480, 490);
    assert_eq!(app.host.chat.unread(1), 1);
    // the window loses the keyboard: what comes is unread again
    assert!(!app.monitor_event(seat, &WindowEvent::Focused(false)));
    focus(&mut app, seat, false);
    nearby(&mut app, 1, 2, "anyone there?");
    assert_eq!(app.host.chat.unread(1), 2);
    // the chat part hidden: the picture has the whole window again
    act(&mut app, Act::Chat(seat, false), now);
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 480, 300);
    assert!(!shows(&drawn, "hello neighbours"));
    // a window too low for both keeps the picture
    act(&mut app, Act::Chat(seat, true), now);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 320, 150);
    assert!(!shows(&drawn, "hello neighbours"), "{drawn:?}");
    assert!(count(&pixels, RED) > 0);
}

fn click(app: &mut Window_, which: Which, (width, height): (u32, u32), at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    draw_with(app, which, width, height, vec![egui::Event::PointerMoved(at), press(true)]);
    draw_with(app, which, width, height, vec![press(false)]);
}

/// A click in a monitor's box: the last row of its window, at its left.
fn click_field(app: &mut Window_, which: Which, size: (u32, u32)) {
    click(app, which, size, egui::pos2(60.0, size.1 as f32 - 18.0));
}

fn type_text(app: &mut Window_, which: Which, (width, height): (u32, u32), text: &str) {
    draw_with(app, which, width, height, vec![egui::Event::Text(text.to_owned())]);
}

fn press_enter(app: &mut Window_, which: Which, (width, height): (u32, u32)) {
    let key = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    draw_with(app, which, width, height, vec![key(true)]);
    draw_with(app, which, width, height, vec![key(false)]);
}

#[test]
fn a_line_typed_in_a_monitor_is_said_once_by_the_character_it_shows() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    act(&mut app, Act::Chat(seat, true), now);
    nearby(&mut app, 1, 1, "hello neighbours");
    nearby(&mut app, 0, 1, "hello to the other one");
    focus(&mut app, seat, true);
    let (which, size) = (Which::Monitor(1), (480, 490));
    draw_in(&mut app, which, size.0, size.1);
    app.host.chat.take_effects();
    click_field(&mut app, which, size);
    type_text(&mut app, which, size, "on my way");
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "on my way");
    press_enter(&mut app, which, size);
    // handed to that character's viewer, in the conversation shown, once
    assert_eq!(sent(&mut app), [(1, Conversation::Nearby, "on my way".to_owned())]);
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "");
    assert_eq!(
        app.host.chat.draft(0, Conversation::Nearby),
        "",
        "nothing of it is the other character's"
    );
    assert_eq!(app.host.chat.sending(1, Conversation::Nearby), 1);
    // drawn again, and Enter again: nothing is said twice
    press_enter(&mut app, which, size);
    draw_in(&mut app, which, size.0, size.1);
    assert!(sent(&mut app).is_empty());
    // no answer (a broken pipe): the line is uncertain, said so in that
    // conversation, and never handed again
    app.host
        .chat
        .tick(Instant::now() + ::folderstorm_session::chat::SEND_WAIT + Duration::from_secs(1));
    let (drawn, _) = draw_in(&mut app, which, size.0, size.1);
    assert!(shows(&drawn, &tr!("folderstorm-chat-uncertain", text = "on my way")), "{drawn:?}");
    assert!(sent(&mut app).is_empty());
    // a text typed in the shared chat for that character is this box's too
    assert!(app.host.chat.set_draft(1, Conversation::Nearby, "typed in the shared chat"));
    let (drawn, _) = draw_in(&mut app, which, size.0, size.1);
    assert!(shows(&drawn, "typed in the shared chat"), "{drawn:?}");
}

#[test]
fn the_chips_fit_two_rows_at_every_width_and_the_rest_is_behind_plus_n() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    let seat = Seat::Slot(1);
    open(&mut app, seat);
    act(&mut app, Act::Chat(seat, true), now);
    nearby(&mut app, 1, 1, "hello");
    many(&mut app, 1, 3);
    app.host.test_picture(1, [480, 270], RED, now);
    // four conversations in a wide window: every name whole
    draw_in(&mut app, Which::Monitor(1), 640, 580);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 640, 580);
    assert!(
        shows(&drawn, "Alexandria Featherstonehaugh") && shows(&drawn, "Christopher Longbottom-Smythe"),
        "{drawn:?}"
    );
    assert!(!drawn.iter().any(|t| t.starts_with('+')), "{drawn:?}");
    keep("chips-640-four", &pixels, 640, 580);
    // narrower: the names shrink together, none goes away
    draw_in(&mut app, Which::Monitor(1), 320, 400);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 320, 400);
    assert!(!shows(&drawn, "Alexandria Featherstonehaugh"), "{drawn:?}");
    assert!(drawn.iter().any(|t| t.starts_with("Alexan") && t.ends_with('…')), "{drawn:?}");
    assert!(shows(&drawn, "Bo Li"), "a short name stays whole: {drawn:?}");
    assert!(!drawn.iter().any(|t| t.starts_with('+')), "{drawn:?}");
    keep("chips-320-four", &pixels, 320, 400);
    // thirteen conversations: two rows, then "+N"
    many(&mut app, 1, 12);
    for (width, height) in [(320, 400), (480, 490), (640, 580)] {
        draw_in(&mut app, Which::Monitor(1), width, height);
        let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), width, height);
        let more = drawn
            .iter()
            .find(|t| t.starts_with('+'))
            .unwrap_or_else(|| panic!("{width}: {drawn:?}"));
        let hidden: usize = more
            .trim_start_matches('+')
            .split(' ')
            .next()
            .and_then(|n| n.parse().ok())
            .expect("a count");
        assert!((1..12).contains(&hidden), "{width}: {more}");
        // (the nearby chat's chip keeps its label)
        assert!(shows(&drawn, tr!("common-nearby-chat")), "{width}: {drawn:?}");
        // no cut name is shorter than six characters and its ellipsis
        let cut: Vec<&String> = drawn.iter().filter(|t| t.ends_with('…') && !t.contains(' ')).collect();
        assert!(cut.iter().all(|t| t.chars().count() >= 7), "{width}: {cut:?}");
        assert!(!shows(&drawn, "use of widget"), "{width}: {drawn:?}");
        keep(&format!("chips-{width}-many"), &pixels, width, height);
    }
    // every hidden one is unread: "+N" says how many
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 320, 400);
    assert!(drawn.iter().any(|t| t.starts_with('+') && t.contains('·')), "{drawn:?}");
    // "+N" opens the list in the lines' place
    app.monitors.pane_mut(seat).more = true;
    draw_in(&mut app, Which::Monitor(1), 320, 400);
    let (drawn, pixels) = draw_in(&mut app, Which::Monitor(1), 320, 400);
    assert!(shows(&drawn, tr!("folderstorm-chips-more-heading")), "{drawn:?}");
    keep("chips-320-list", &pixels, 320, 400);
    // nothing is read while the list is in the lines' place
    focus(&mut app, seat, true);
    let unread = app.host.chat.unread(1);
    draw_in(&mut app, Which::Monitor(1), 320, 400);
    assert_eq!(app.host.chat.unread(1), unread);
    // picking one from the list shows it, with a chip
    let hidden = Conversation::Session(2, 100);
    let (mut shown, mut more) = (Conversation::Nearby, true);
    let pick = chips::Asked {
        select: Some(hidden),
        ..Default::default()
    };
    assert!(pick.apply(&mut app.host.chat, 1, &mut shown, &mut more));
    assert_eq!((shown, more), (hidden, false));
    app.monitors.pane_mut(seat).shown[1] = hidden;
    app.monitors.pane_mut(seat).more = false;
    let (drawn, _) = draw_in(&mut app, Which::Monitor(1), 320, 400);
    assert!(
        drawn.iter().any(|t| t.starts_with("Alexan")),
        "the one on screen has its chip: {drawn:?}"
    );
    // closing it in the host: its chip goes, the nearby chat is shown, and
    // its viewer is told nothing
    app.host.chat.take_effects();
    let close = chips::Asked {
        close: Some(hidden),
        ..Default::default()
    };
    assert!(close.apply(&mut app.host.chat, 1, &mut shown, &mut more));
    assert_eq!(shown, Conversation::Nearby);
    assert!(app.host.chat.open_thread(1, hidden).is_none());
    assert!(app.host.chat.take_effects().is_empty());
}

#[test]
fn the_shared_chat_has_the_same_chips() {
    use ::folderstorm_session::panels::Panel;
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    nearby(&mut app, 1, 1, "hello");
    many(&mut app, 1, 12);
    app.host.desk.set_panel_out(Panel::Chat, true);
    app.panes[Panel::Chat.index()] = Some(app.look.view());
    let chat = Which::Pane(Panel::Chat);
    // the second character's tab: found by its alias, then clicked
    draw_in(&mut app, chat, 620, 640);
    let tab = (40..400).step_by(8).map(|x| egui::pos2(x as f32, 88.0)).find(|at| {
        click(&mut app, chat, (620, 640), *at);
        let (drawn, _) = draw_in(&mut app, chat, 620, 640);
        drawn.iter().any(|t| t.starts_with('+'))
    });
    assert!(tab.is_some(), "the second character's tab shows its chips");
    let (drawn, pixels) = draw_in(&mut app, chat, 620, 640);
    keep("shared-chat-620-many", &pixels, 620, 640);
    assert!(shows(&drawn, tr!("common-nearby-chat")), "{drawn:?}");
    assert!(drawn.iter().any(|t| t.starts_with('+')), "two rows, then +N: {drawn:?}");
    // narrower: still "+N"
    draw_in(&mut app, chat, 380, 640);
    let (drawn, pixels) = draw_in(&mut app, chat, 380, 640);
    assert!(drawn.iter().any(|t| t.starts_with('+')), "{drawn:?}");
    assert!(!shows(&drawn, "use of widget"), "{drawn:?}");
    keep("shared-chat-380-many", &pixels, 380, 640);
    // wide enough for all of them at a shorter length: no "+N"
    draw_in(&mut app, chat, 1400, 640);
    let (drawn, pixels) = draw_in(&mut app, chat, 1400, 640);
    assert!(!drawn.iter().any(|t| t.starts_with('+')), "{drawn:?}");
    keep("shared-chat-1400-many", &pixels, 1400, 640);
}

#[test]
fn the_cycling_monitor_shows_each_background_character_in_turn() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 4, now);
    let seat = Seat::Cycling;
    open(&mut app, seat);
    // its first look: the first character behind the active one
    app.monitors_watch(now);
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    let (drawn, _) = draw_in(&mut app, Which::Cycling, 480, 300);
    assert!(shows(&drawn, "Shop keeper"), "the header says who: {drawn:?}");
    // drawn: seen; only that character is asked
    app.monitors_watch(now);
    assert!(matches!(app.host.desk.cycling_watch(1), Watch::Asked { .. }));
    assert_eq!(app.host.desk.cycling_watch(2), Watch::Hidden);
    assert_eq!(app.host.desk.watch(1), Watch::Closed, "its own monitor is not open");
    app.host.test_picture(1, [480, 270], RED, now);
    app.host.tick_monitors(now);
    assert!(app.host.screens[1].shown.is_some(), "the cycling monitor's picture is kept");
    let (_, pixels) = draw_in(&mut app, Which::Cycling, 480, 300);
    assert_eq!(count(&pixels, RED), 480 * 270);
    keep("cycling-480", &pixels, 480, 300);
    // 5 s later: the next one; the first one's picture is not the next's
    let later = now + Duration::from_secs(5);
    app.monitors_watch(later);
    assert_eq!(app.monitors.cycle.shown(), Some(2));
    let (drawn, pixels) = draw_in(&mut app, Which::Cycling, 480, 300);
    assert!(shows(&drawn, "Kim Resident"), "{drawn:?}");
    assert_eq!(count(&pixels, RED), 0);
    app.monitors_watch(later);
    assert!(matches!(app.host.desk.cycling_watch(2), Watch::Asked { .. }));
    assert_eq!(app.host.desk.cycling_watch(1), Watch::Hidden);
    // nobody shows the first one any more: its picture is dropped
    app.host.tick_monitors(later);
    assert!(app.host.screens[1].shown.is_none());
    // previous, next and pause
    act(&mut app, Act::Step(false), later);
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    act(&mut app, Act::Step(true), later);
    act(&mut app, Act::Step(true), later);
    assert_eq!(app.monitors.cycle.shown(), Some(3));
    act(&mut app, Act::Pause(true), later);
    app.monitors_watch(later + Duration::from_secs(60));
    assert_eq!(app.monitors.cycle.shown(), Some(3));
    act(&mut app, Act::Pause(false), later);
    // its Switch: the one shown comes to the front, and the cycling
    // monitor shows the one that left it
    draw_in(&mut app, Which::Cycling, 480, 300);
    act(&mut app, Act::Switch(seat, 3), later);
    assert_eq!(app.host.desk.active(), Some(3));
    assert_eq!(app.monitors.cycle.shown(), Some(0));
    let (drawn, _) = draw_in(&mut app, Which::Cycling, 480, 300);
    assert!(shows(&drawn, "Main"), "{drawn:?}");
    // the interval is a setting
    act(&mut app, Act::Interval(30), later);
    app.monitors_watch(later + Duration::from_secs(10));
    assert_eq!(app.monitors.cycle.shown(), Some(0));
    app.monitors_watch(later + Duration::from_secs(31));
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    // its window covered: nobody is asked
    draw_in(&mut app, Which::Cycling, 480, 300);
    cover(&mut app, seat, true);
    app.monitors_watch(later + Duration::from_secs(32));
    assert_eq!(app.host.desk.cycling_watch(1), Watch::Hidden);
    cover(&mut app, seat, false);
    // its options have the turn and "Pause cycling while typing"
    act(&mut app, Act::Options(seat, true), later);
    draw_in(&mut app, Which::Cycling, 480, 400);
    let (drawn, pixels) = draw_in(&mut app, Which::Cycling, 480, 400);
    assert!(
        shows(&drawn, tr!("folderstorm-cycling-pause-typing")) && shows(&drawn, tr!("folderstorm-cycling-interval")),
        "{drawn:?}"
    );
    assert!(!shows(&drawn, "use of widget"), "{drawn:?}");
    keep("cycling-480-options", &pixels, 480, 400);
}

#[test]
fn the_cycling_monitor_says_so_when_nobody_is_behind() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 1, now);
    // (the demo's second character is not started)
    open(&mut app, Seat::Cycling);
    app.monitors_watch(now);
    assert_eq!(app.monitors.cycle.shown(), None);
    let (drawn, pixels) = draw_in(&mut app, Which::Cycling, 480, 300);
    assert!(shows(&drawn, tr!("folderstorm-cycling-nobody")), "{drawn:?}");
    keep("cycling-480-nobody", &pixels, 480, 300);
    app.monitors_watch(now);
    assert_eq!(app.cycling_sight(), None);
}

#[test]
fn the_cycling_monitor_waits_while_the_user_types_in_its_chat() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 3, now);
    let seat = Seat::Cycling;
    open(&mut app, seat);
    act(&mut app, Act::Chat(seat, true), now);
    for slot in 0..3 {
        nearby(&mut app, slot, 1, "hello");
    }
    focus(&mut app, seat, true);
    let (which, size) = (Which::Cycling, (480, 490));
    app.monitors_watch(now);
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    draw_in(&mut app, which, size.0, size.1);
    app.host.chat.take_effects();
    // the user clicks in the box and types, slowly: far longer than a turn
    click_field(&mut app, which, size);
    type_text(&mut app, which, size, "wait for");
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "wait for");
    for seconds in 1..40 {
        app.monitors_watch(now + Duration::from_secs(seconds));
        assert_eq!(app.monitors.cycle.shown(), Some(1), "{seconds} s");
    }
    // the header says why it does not move
    let (_, pixels) = draw_in(&mut app, which, size.0, size.1);
    assert!(count(&pixels, app.palette.violet) > 100, "the pause is lit");
    keep("cycling-480-typing", &pixels, size.0, size.1);
    // the window loses the keyboard with the text unsent: it still waits
    focus(&mut app, seat, false);
    app.monitors_watch(now + Duration::from_secs(60));
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    focus(&mut app, seat, true);
    // "next" is the user's to press: the text no longer holds the turn,
    // and stays the draft of the character it was typed for
    act(&mut app, Act::Step(true), now + Duration::from_secs(60));
    assert_eq!(app.monitors.cycle.shown(), Some(2));
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "wait for");
    act(&mut app, Act::Step(false), now + Duration::from_secs(60));
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    draw_in(&mut app, which, size.0, size.1);
    // more text, then Enter: said by the character it was typed for
    type_text(&mut app, which, size, " me");
    let sending = now + Duration::from_secs(61);
    app.monitors_watch(sending);
    press_enter(&mut app, which, size);
    assert_eq!(sent(&mut app), [(1, Conversation::Nearby, "wait for me".to_owned())]);
    // sent: a whole turn is left before the next character
    draw_in(&mut app, which, size.0, size.1);
    app.monitors_watch(sending + Duration::from_secs(1));
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    app.monitors_watch(sending + Duration::from_secs(4));
    assert_eq!(app.monitors.cycle.shown(), Some(1));
    app.monitors_watch(sending + Duration::from_secs(7));
    assert_eq!(app.monitors.cycle.shown(), Some(2));
}

#[test]
fn with_the_pause_off_a_text_stays_with_its_character_and_is_never_sent_as_another() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 3, now);
    let seat = Seat::Cycling;
    open(&mut app, seat);
    act(&mut app, Act::Chat(seat, true), now);
    act(&mut app, Act::PauseTyping(false), now);
    for slot in 0..3 {
        nearby(&mut app, slot, 1, "hello");
    }
    focus(&mut app, seat, true);
    let (which, size) = (Which::Cycling, (480, 490));
    app.monitors_watch(now);
    draw_in(&mut app, which, size.0, size.1);
    app.host.chat.take_effects();
    click_field(&mut app, which, size);
    type_text(&mut app, which, size, "for the first");
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "for the first");
    // the turn passes under the text
    app.monitors_watch(now + Duration::from_secs(5));
    assert_eq!(app.monitors.cycle.shown(), Some(2));
    // the box is the next character's own, empty: the text is not shown
    // under its name, and Enter says nothing
    let (drawn, _) = draw_in(&mut app, which, size.0, size.1);
    assert!(shows(&drawn, "Kim Resident") && !shows(&drawn, "for the first"), "{drawn:?}");
    press_enter(&mut app, which, size);
    assert!(sent(&mut app).is_empty());
    // whatever is typed and sent now is the character's whose name the
    // header shows, and never the first text
    click_field(&mut app, which, size);
    type_text(&mut app, which, size, "x");
    press_enter(&mut app, which, size);
    let said: Vec<_> = sent(&mut app);
    assert!(said.iter().all(|(slot, _, text)| *slot == 2 && text != "for the first"), "{said:?}");
    // the text is still the first character's, in its own conversation
    assert_eq!(app.host.chat.draft(1, Conversation::Nearby), "for the first");
}

#[test]
fn closing_a_monitor_s_window_closes_that_monitor_and_nothing_else() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 3, now);
    open(&mut app, Seat::Slot(1));
    open(&mut app, Seat::Slot(2));
    open(&mut app, Seat::Cycling);
    draw_in(&mut app, Which::Monitor(1), 480, 300);
    app.monitors_watch(now);
    app.host.desk.take_effects();
    // the window's close button
    assert!(app.monitor_event(Seat::Slot(1), &WindowEvent::CloseRequested));
    assert!(app.monitors.view(Seat::Slot(1)).is_none());
    assert!(app.host.desk.slot(1).is_some_and(|s| s.monitor.is_none()));
    assert!(app.monitors.view(Seat::Slot(2)).is_some() && app.monitors.view(Seat::Cycling).is_some());
    assert!(!app.host.exit && !app.closing);
    assert_eq!(app.host.desk.live(), 3);
    assert_eq!(app.host.desk.active(), Some(0));
    // its viewer is a plain background character again; nothing is ended
    let effects = app.host.desk.take_effects();
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Send { slot: 1, message: ToViewer::Assign(a), .. } if a.role == Role::Background
    )));
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Kill { .. } | Effect::Exit)));
    // the cycling monitor's: closed, the others stay
    assert!(app.monitor_event(Seat::Cycling, &WindowEvent::CloseRequested));
    assert!(!app.host.desk.monitors().cycling.open);
    assert!(app.monitors.view(Seat::Slot(2)).is_some());
    // "Close all monitors" from the bar
    open(&mut app, Seat::Cycling);
    app.apply(vec![HostAct::Bar(::folderstorm_session::bar::Request::CloseMonitors)], now);
    assert!(app.host.desk.slots().all(|(_, s)| s.monitor.is_none()) && !app.host.desk.monitors().cycling.open);
}

#[test]
fn the_monitors_page_has_the_defaults_and_the_cycling_monitor() {
    use ::folderstorm_session::bar::{Page, Request};
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, 2, now);
    // "Monitor settings…" from the bar: the settings window's section
    app.apply(vec![HostAct::Bar(Request::Show(Page::Monitors))], now);
    assert_eq!(
        app.host.desk.settings_window().section,
        ::folderstorm_session::settings::Section::Monitors
    );
    app.settings.view = Some(app.look.view());
    draw_in(&mut app, Which::Settings, 1000, 760);
    let (drawn, pixels) = draw_in(&mut app, Which::Settings, 1000, 760);
    for text in [
        tr!("folderstorm-settings-monitors"),
        tr!("folderstorm-monitors-page-defaults"),
        tr!("folderstorm-monitor-budget-note"),
        tr!("folderstorm-cycling-open"),
        tr!("folderstorm-cycling-pause-typing"),
        "480 × 270",
    ] {
        assert!(shows(&drawn, text), "{text}: {drawn:?}");
    }
    assert!(!shows(&drawn, "use of widget"), "two widgets share an id: {drawn:?}");
    keep("monitors-page", &pixels, 1000, 760);
    // its choices
    act(&mut app, Act::Cycling(true), now);
    assert!(app.host.desk.monitors().cycling.open);
    act(&mut app, Act::PauseTyping(false), now);
    act(&mut app, Act::Interval(10), now);
    let cycling = app.host.desk.monitors().cycling;
    assert!(!cycling.pause_typing && cycling.interval_s == 10);
    let large = Spec {
        size: Size::Large,
        rate: Rate::Half,
    };
    act(&mut app, Act::Spec(Seat::Cycling, large), now);
    assert_eq!(app.host.desk.monitors().cycling.spec, large);
}

/// Every icon this file names is one the viewer embeds.
#[test]
fn the_monitors_icons_exist() {
    for name in [
        "arrows-exchange",
        "message-circle-filled",
        "pinned-filled",
        "pin-filled",
        "settings-filled",
        "chevron-left",
        "chevron-right",
        "player-pause-filled",
        "player-play-filled",
        "alert-triangle-filled",
        "arrow-left",
        "check",
        "repeat",
        "device-desktop-filled",
    ] {
        assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
    }
    assert_eq!(Seat::Slot(1).key(), "monitor-2");
    assert_eq!(Seat::Cycling.key(), "cycling");
    assert_eq!(Seat::of(Seat::Slot(3).which()), Some(Seat::Slot(3)));
    assert_eq!(Seat::of(Which::Main), None);
    assert_ne!(Seat::Slot(0).look_key(), ::folderstorm_session::chat::SHARED_WINDOW);
}
