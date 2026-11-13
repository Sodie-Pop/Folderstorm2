//! The settings window drawn by the processor's painter, without a window:
//! every section at a narrow and a wide size, the ways to it, and what its
//! choices do. `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps each picture,
//! as `<folder>/<name>.png`, to be looked at.

use super::super::super::chat_ui::tests::{heard, host};
use super::super::panels::Which;
use super::super::tests::{draw, draw_in, window};
use super::*;
use ::folderstorm_session::identity::Colour;
use ::folderstorm_session::ipc::{AttentionItem, AttentionKind, Conversation, LineKind, Screen, Status};
use std::time::{Duration, Instant};

fn shows(drawn: &[String], text: &str) -> bool {
    drawn.iter().any(|t| t.contains(text))
}

fn painted(pixels: &[u32]) -> usize {
    let background = pixels[0];
    pixels.iter().filter(|p| **p != background).count()
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

/// Three characters, the first two in the world with colours and an
/// alias, the first active.
fn running(app: &mut Window_, now: Instant) {
    app.host.desk.add("demo-3").expect("a third character");
    for slot in 0..2 {
        app.host.desk.launch(slot, slot == 0);
        app.host.desk.connected(slot, 1, now);
        let status = Status {
            screen: Screen::World,
            name: ["Jane Doe", "Sam Resident"][slot].to_owned(),
            ..Default::default()
        };
        app.host.desk.status(slot, 1, status, now);
        // (with its heartbeat, as over a live link: a slow run stays within
        // the silence limit)
        app.host.desk.heartbeat(slot, 1, 0, now);
    }
    app.host.desk.set_colour(0, Colour::Teal);
    app.host.desk.set_colour(1, Colour::Orange);
    assert_eq!(app.host.desk.set_alias(0, "Main"), Ok(()));
    app.host.desk.take_effects();
}

/// The settings window open on that section, as `sync_settings` opens it.
fn open(app: &mut Window_, section: Section, slot: Option<usize>) {
    app.apply(vec![HostAct::Settings(Act::Open(section, slot))], Instant::now());
    if app.settings.view.is_none() {
        app.settings.view = Some(app.look.view());
    }
}

#[test]
fn every_section_is_drawn_whole_at_a_narrow_and_a_wide_size() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    for section in Section::ALL {
        open(&mut app, section, None);
        for (width, height) in [(520, 700), (1000, 760)] {
            draw_in(&mut app, Which::Settings, width, height);
            let (drawn, pixels) = draw_in(&mut app, Which::Settings, width, height);
            // the list of the sections, and the page's heading
            for each in Section::ALL {
                assert!(shows(&drawn, section_name(each)), "{section:?} at {width}: {}", section_name(each));
            }
            assert!(shows(&drawn, section_intro(section)), "{section:?} at {width}: {drawn:?}");
            assert!(!shows(&drawn, "use of widget"), "two widgets share an id in {section:?}: {drawn:?}");
            assert!(painted(&pixels) > 8_000, "{section:?} at {width}: {} pixels", painted(&pixels));
            keep(&format!("settings-{}-{width}", section.key()), &pixels, width, height);
        }
    }
    // what each section holds
    let holds: [(Section, &[&str]); 8] = [
        (
            Section::Toolbar,
            &[
                tr!("folderstorm-bar-page-where"),
                tr!("folderstorm-bar-page-toolbar-labels"),
                tr!("folderstorm-bar-page-quick-labels"),
                tr!("folderstorm-bar-page-hide-host"),
                tr!("folderstorm-bar-page-taskbar"),
                tr!("folderstorm-host-stage"),
                tr!("folderstorm-bar-page-quick"),
            ],
        ),
        (
            Section::Characters,
            &[
                tr!("folderstorm-settings-logged-in-as"),
                "Jane Doe",
                tr!("folderstorm-alias"),
                tr!("folderstorm-colour"),
                tr!("folderstorm-settings-warm"),
                tr!("folderstorm-host-sound-all"),
                tr!("folderstorm-settings-include-attention"),
                tr!("folderstorm-settings-move-down"),
                tr!("folderstorm-settings-ims"),
                tr!("folderstorm-settings-notification-alerts"),
                tr!("folderstorm-settings-preview"),
            ],
        ),
        (
            Section::Chat,
            &[
                tr!("folderstorm-settings-nearby"),
                tr!("folderstorm-settings-conferences"),
                tr!("folderstorm-settings-notices"),
                tr!("folderstorm-settings-teleports"),
                tr!("folderstorm-settings-permissions"),
                tr!("folderstorm-settings-inventory"),
                tr!("folderstorm-settings-newest-first"),
                tr!("folderstorm-settings-open-inbox"),
                tr!("folderstorm-settings-mark-displayed"),
                tr!("folderstorm-settings-auto-read"),
                "60",
            ],
        ),
        (
            Section::Sound,
            &[
                tr!("folderstorm-bar-voice-long"),
                tr!("folderstorm-bar-mute-long"),
                tr!("folderstorm-chat-sound"),
                "1: Main",
                "3: demo-3",
            ],
        ),
        (
            Section::Monitors,
            &[tr!("folderstorm-monitors-page-defaults"), tr!("folderstorm-cycling-pause-typing")],
        ),
        (Section::Hotkeys, &[tr!("folderstorm-hotkeys-enabled"), "Ctrl+Shift+F1"]),
        (
            Section::Advanced,
            &[
                tr!("folderstorm-settings-open-folder"),
                tr!("folderstorm-settings-recover"),
                tr!("folderstorm-settings-show-diagnostics"),
                tr!("folderstorm-settings-credits-tabler"),
            ],
        ),
        (Section::Updates, &[tr!("bug-title"), tr!("ui-options-page-system")]),
    ];
    for (section, texts) in holds {
        open(&mut app, section, None);
        draw_in(&mut app, Which::Settings, 1000, 900);
        let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 900);
        for text in texts {
            assert!(shows(&drawn, text), "{section:?}: {text} in {drawn:?}");
        }
    }
}

#[test]
fn the_ways_to_the_window_open_it_on_their_section() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    assert!(!app.settings_wanted());
    // the host's own window: "Settings…" under the cards
    let (drawn, _) = draw(&mut app, 1120, 760);
    assert!(shows(&drawn, tr!("folderstorm-settings-open")), "{drawn:?}");
    assert!(
        !shows(&drawn, tr!("folderstorm-host-stage")) && !shows(&drawn, tr!("folderstorm-chat-sound")),
        "those boxes moved to the settings: {drawn:?}"
    );
    assert!(
        !shows(&drawn, tr!("folderstorm-bar-settings-bar")),
        "the old pages' buttons are gone"
    );
    // a card's palette: Characters, on that character
    open(&mut app, Section::Characters, Some(1));
    assert!(app.settings_wanted() && app.settings.raise);
    assert_eq!(app.host.desk.settings_window().section, Section::Characters);
    draw_in(&mut app, Which::Settings, 1000, 760);
    let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 760);
    assert!(shows(&drawn, "2: Sam Resident") && shows(&drawn, "Sam Resident"), "{drawn:?}");
    // another character, picked at the top
    app.apply(vec![HostAct::Settings(Act::Character(2))], now);
    let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 760);
    assert!(
        shows(&drawn, "3: demo-3") && shows(&drawn, tr!("folderstorm-settings-not-logged-in")),
        "{drawn:?}"
    );
    // a section of the list
    app.apply(vec![HostAct::Settings(Act::Section(Section::Advanced))], now);
    assert_eq!(app.host.desk.settings_window().section, Section::Advanced);
    // closing it closes it, and nothing else
    let live = app.host.desk.live();
    assert!(app.settings_event(&WindowEvent::CloseRequested));
    assert!(!app.settings_wanted() && app.settings.view.is_none());
    assert_eq!(app.host.desk.live(), live);
}

#[test]
fn what_the_chats_show_and_what_attention_lists_follow_the_settings() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    let jane = Conversation::Session(0, 0x4a);
    heard(&mut app.host, 1, jane, 1, LineKind::Said, "Jane Doe", "hello");
    heard(&mut app.host, 1, Conversation::Nearby, 1, LineKind::Said, "Kim", "near");
    app.host.tick(now);
    assert_eq!(app.host.chat.unread(1), 2);
    // IMs off: in no chip, counted nowhere
    let shown = ::folderstorm_session::settings::ChatShown {
        ims: false,
        ..Default::default()
    };
    app.apply(vec![HostAct::Settings(Act::ChatShown(shown))], now);
    assert_eq!(app.host.chat.unread(1), 1);
    assert!(app.host.chat.open_thread(1, jane).is_none());
    assert!(app.host.unsaved.is_some(), "saved");
    // the inbox: a character left out, then a kind left out
    app.host.sample_inbox = true;
    app.host.tick(now);
    let all = app.host.inbox.count();
    assert!(all > 0, "the demo's sample questions are listed");
    app.apply(vec![HostAct::Settings(Act::Included(1, false))], now);
    assert!(app.host.inbox.waiting().iter().all(|w| w.slot != 1));
    assert_eq!(app.host.inbox.count_for(1), 0);
    let attention = ::folderstorm_session::settings::Attention {
        teleports: false,
        ..Default::default()
    };
    app.apply(vec![HostAct::Settings(Act::Attention(attention))], now);
    assert!(app.host.inbox.waiting().iter().all(|w| w.item.kind != AttentionKind::Teleport));
    assert_eq!(app.host.bar_state().inbox, app.host.inbox.count(), "the bar counts what is listed");
    // the newest first: the inbox's rows reversed
    let item = |id, title: &str| AttentionItem {
        id,
        kind: AttentionKind::Inventory,
        title: title.into(),
        text: String::new(),
    };
    app.host.sample_inbox = false;
    app.host.inbox.set(0, app.host.chat.login(0), &[item(1, "first one")], 0, now);
    app.host.inbox.set(
        0,
        app.host.chat.login(0),
        &[item(1, "first one"), item(2, "second one")],
        0,
        now + Duration::from_secs(5),
    );
    let order = |app: &mut Window_| {
        let (drawn, _) = draw(app, 1120, 900);
        let at = |text: &str| drawn.iter().position(|t| t.contains(text));
        (at("first one"), at("second one"))
    };
    if let (Some(first), Some(second)) = order(&mut app) {
        assert!(first < second, "oldest first");
        app.host.desk.set_attention(::folderstorm_session::settings::Attention {
            newest_first: true,
            ..attention
        });
        let (first, second) = order(&mut app);
        assert!(second < first, "newest first");
    }
}

#[test]
fn the_active_character_is_read_in_the_host_after_the_delay() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    app.host.desk.switch(0, now);
    app.host.tick(now);
    assert_eq!(app.host.desk.active(), Some(0));
    let jane = Conversation::Session(0, 0x4a);
    heard(&mut app.host, 0, jane, 1, LineKind::Said, "Jane Doe", "hello");
    app.host.tick(now + Duration::from_secs(1));
    assert_eq!(app.host.chat.unread(0), 1);
    app.host.tick(now + Duration::from_secs(30));
    assert_eq!(app.host.chat.unread(0), 1);
    app.host.tick(now + Duration::from_secs(61));
    assert_eq!(app.host.chat.unread(0), 0, "read after 60 s in front");
    // off: never
    app.apply(
        vec![HostAct::Settings(Act::Reading(::folderstorm_session::settings::Reading {
            auto_read: false,
            ..Default::default()
        }))],
        now,
    );
    heard(&mut app.host, 0, jane, 2, LineKind::Said, "Jane Doe", "again");
    app.host.tick(now + Duration::from_secs(600));
    assert_eq!(app.host.chat.unread(0), 1);
}

#[test]
fn a_character_s_alerts_decide_what_sounds() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    app.host.desk.switch(0, now);
    app.host.tick(now);
    let jane = Conversation::Session(0, 0x4a);
    heard(&mut app.host, 1, jane, 1, LineKind::Said, "Jane Doe", "for the one behind");
    app.host.tick(now);
    assert!(std::mem::take(&mut app.host.alert), "an IM to a background character");
    let alerts = ::folderstorm_session::settings::Alerts {
        ims: false,
        ..Default::default()
    };
    app.apply(vec![HostAct::Settings(Act::Alerts(1, alerts))], now);
    app.host.alert = false;
    heard(&mut app.host, 1, jane, 2, LineKind::Said, "Jane Doe", "again");
    app.host.tick(now);
    assert!(!app.host.alert, "its IMs are quiet now");
    assert_eq!(app.host.desk.slot(1).map(|s| s.alerts), Some(alerts));
}

#[test]
fn two_cards_swap_from_the_characters_section() {
    let now = Instant::now();
    let mut app = window(host());
    // nothing runs: the cards may move
    app.host.desk.add("demo-3").expect("a third character");
    app.host.desk.set_colour(2, Colour::Violet);
    open(&mut app, Section::Characters, Some(2));
    app.apply(vec![HostAct::Settings(Act::Swap(2, 1))], now);
    assert_eq!(app.host.desk.slot(1).map(|s| s.profile.shown().to_owned()), Some("demo-3".into()));
    assert_eq!(app.host.desk.slot(1).map(|s| s.colour), Some(Colour::Violet));
    assert_eq!(app.settings.slot, 1, "the section follows the character");
    // a running character does not move
    app.host.desk.launch(0, false);
    app.apply(vec![HostAct::Settings(Act::Swap(1, 0))], now);
    assert_eq!(app.host.desk.slot(0).map(|s| s.profile.shown().to_owned()), Some("demo-1".into()));
}

#[test]
fn the_diagnostics_and_the_credits_show_when_asked() {
    let now = Instant::now();
    let mut app = window(host());
    running(&mut app, now);
    open(&mut app, Section::Advanced, None);
    app.apply(
        vec![HostAct::Settings(Act::Diagnostics(true)), HostAct::Settings(Act::Credits(true))],
        now,
    );
    draw_in(&mut app, Which::Settings, 1000, 1400);
    let (drawn, pixels) = draw_in(&mut app, Which::Settings, 1000, 1400);
    assert!(
        shows(&drawn, "1: demo-1") && shows(&drawn, "Healthy") && shows(&drawn, tr!("folderstorm-host-in-world")),
        "{drawn:?}"
    );
    assert!(shows(&drawn, "Permission is hereby granted"), "the Tabler licence");
    keep("settings-advanced-open", &pixels, 1000, 1400);
    // with no window of the host, nothing to recover
    app.apply(vec![HostAct::Settings(Act::Recover)], now);
    assert_eq!(app.settings.recovered, Some(0));
}

#[test]
fn the_settings_icons_exist() {
    let mut names: Vec<&str> = Section::ALL.into_iter().map(section_icon).collect();
    names.extend([
        "users",
        "arrow-up",
        "arrow-down",
        "player-play-filled",
        "refresh",
        "bell-filled",
        "folder-filled",
        "app-window-filled",
        "settings-filled",
    ]);
    for name in names {
        assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
    }
}
