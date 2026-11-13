//! The desk's side of checkpoint 8: the bar's settings among the host's,
//! what the bar shows of the desk, what its clicks do, "Mute background
//! sound", "Voice follows active" and the one taskbar button.

use super::*;
use crate::bar::{LabelMode, Page, Placement, Presence, QuickAction, Request};
use crate::identity::Colour;
use crate::panels::Panel;

/// `count` characters, the first `running` of them in the world (the
/// first one active), every assignment acknowledged.
fn desk(count: usize, running: usize) -> (Desk, Instant) {
    let now = Instant::now();
    let mut d = Desk::new(Config::demo(count));
    for slot in 0..running {
        d.launch(slot, slot == 0);
        d.connected(slot, 1, now);
    }
    for slot in 0..running {
        report(&mut d, slot, now);
    }
    d.take_effects();
    (d, now)
}

/// The viewer reports, in the world, holding what was sent to it.
fn report(d: &mut Desk, slot: usize, now: Instant) {
    let s = d.slot(slot).expect("slot");
    let (launch, sent) = (s.launch(), s.sent.expect("sent").0);
    let status = Status {
        assign_seq: sent.seq,
        screen: Screen::World,
        name: s.status.as_ref().map(|st| st.name.clone()).unwrap_or_default(),
        sound_muted: sent.sound_muted,
        voice_allowed: sent.voice_allowed,
        ..Default::default()
    };
    d.status(slot, launch, status, now);
}

fn settle(d: &mut Desk, now: Instant) {
    // (twice: a voice given up is given at the next report)
    for _ in 0..2 {
        let running: Vec<usize> = d.slots().filter(|(_, s)| s.phase == Phase::Running).map(|(i, _)| i).collect();
        for slot in running {
            report(d, slot, now);
        }
    }
}

fn assigned(d: &Desk, slot: usize) -> Assignment {
    d.slot(slot).and_then(|s| s.sent).expect("an assignment").0
}

fn voices(d: &Desk) -> Vec<usize> {
    d.slots()
        .filter(|(_, s)| s.sent.is_some_and(|(a, _)| a.voice_allowed))
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn a_file_of_before_reads_as_docked_nothing_more_muted_and_voice_following() {
    let old = r#"{"slots":[{"slot":0,"profile":"main-alt"}],"stage":true,"condensed":true}"#;
    let config = serde_json::from_str::<Config>(old).expect("reads").cleaned();
    assert_eq!(config.bar, bar::Config::default());
    assert!(!config.mute_background && config.voice_follows && config.condensed);
    let d = Desk::new(config);
    assert_eq!(d.bar().placement, Placement::Docked);
    assert!(!d.mute_background() && d.voice_follows());
}

#[test]
fn the_bar_s_settings_are_saved_and_read_back() {
    let (mut d, _) = desk(2, 0);
    assert!(d.edit_bar(|bar| {
        bar.placement = Placement::Popped;
        bar.pinned = true;
        bar.quick_labels = LabelMode::TextOnly;
        bar.set_quick(7, QuickAction::MuteBackground);
    }));
    d.set_mute_background(true);
    d.set_voice_follows(false);
    assert_eq!(d.take_effects().iter().filter(|e| matches!(e, Effect::Save)).count(), 3);
    // the same again: nothing to write
    assert!(!d.edit_bar(|bar| bar.pinned = true));
    d.set_mute_background(true);
    d.set_voice_follows(false);
    assert!(d.take_effects().is_empty());
    let text = serde_json::to_string(&d.config()).expect("json");
    assert!(
        text.contains("\"placement\":\"popped\"") && text.contains("\"mute_background\":true"),
        "{text}"
    );
    assert!(
        text.contains("\"voice_follows\":false") && text.contains("\"mute-background\""),
        "{text}"
    );
    let next = Desk::new(serde_json::from_str(&text).expect("json"));
    assert_eq!(next.bar(), d.bar());
    assert!(next.mute_background() && !next.voice_follows());
    // an edit is cleaned as the file is: an action shows once
    assert!(d.edit_bar(|bar| bar.quick[0] = QuickAction::Attention));
    assert_eq!(d.bar().quick.iter().filter(|a| **a == QuickAction::Attention).count(), 1);
}

#[test]
fn the_bar_shows_each_character_with_its_alias_colour_and_where_it_is() {
    let (mut d, now) = desk(4, 2);
    d.set_colour(1, Colour::Teal);
    assert_eq!(d.set_alias(1, "Shop"), Ok(()));
    // the third one waits at its login screen
    d.launch(2, false);
    d.connected(2, 1, now);
    let launch = d.slot(2).expect("slot").launch();
    d.status(
        2,
        launch,
        Status {
            screen: Screen::Login,
            ..Default::default()
        },
        now,
    );
    d.set_monitor(1, Some(monitor::Spec::default()));
    let state = d.bar_state(3, 7);
    assert!(state.docked && !state.collapsed);
    assert_eq!((state.inbox, state.chat_unread, state.chat_open), (3, 7, false));
    assert!(!state.mute_background && state.voice_follows);
    assert_eq!(state.quick, d.bar().quick);
    // over the world viewport, until the settings say span the window
    assert!(!state.span_window);
    assert!(d.edit_bar(|bar| bar.span_window = true));
    assert!(d.bar_state(3, 7).span_window);
    assert!(d.edit_bar(|bar| bar.span_window = false));
    let seen: Vec<_> = state
        .characters
        .iter()
        .map(|c| (c.slot, c.name.as_str(), c.real.as_deref(), c.presence, c.quick, c.monitor))
        .collect();
    assert_eq!(
        seen,
        [
            (0, "demo-1", None, Presence::Active, Quick::Show, false),
            (1, "Shop", Some("demo-2"), Presence::Inactive, Quick::Switch, true),
            (2, "demo-3", None, Presence::Out, Quick::LogIn, false),
            (3, "demo-4", None, Presence::Out, Quick::Launch, false),
        ]
    );
    assert_eq!(state.characters[1].colour, Colour::Teal);
    assert_eq!(state.active().map(|c| c.slot), Some(0));
    assert_eq!(state.next_login().map(|c| c.slot), Some(2));
    // three viewers have a window to draw it in
    assert_eq!(d.bars_shown(), 3);
    d.close(1);
    assert_eq!(d.bars_shown(), 2, "one that is closing is not counted on");
    d.bar_request(Request::Dock(false), now);
    assert_eq!(d.bars_shown(), 0);
    assert!(!d.bar_state(0, 0).docked);
}

#[test]
fn a_pick_is_the_character_s_quick_action_and_never_a_second_process() {
    let (mut d, now) = desk(3, 2);
    // behind: it becomes the active one
    d.bar_request(Request::Pick(1), now);
    assert_eq!(d.active(), Some(1));
    // no viewer: one is started, once
    d.bar_request(Request::Pick(2), now);
    d.bar_request(Request::Pick(2), now);
    let spawns = d.take_effects().into_iter().filter(|e| matches!(e, Effect::Spawn { .. })).count();
    assert_eq!(spawns, 1);
    // a slot there is not, and one without a character
    d.bar_request(Request::Pick(9), now);
    let (mut empty, now) = desk(1, 1);
    empty.bar_request(Request::Pick(3), now);
    assert!(empty.take_effects().is_empty());
}

#[test]
fn open_a_login_only_ever_opens_a_character_that_is_not_logged_in() {
    let (mut d, now) = desk(4, 2);
    // the first one not logged in: the third
    d.bar_request(Request::OpenLogin(None), now);
    let effects = d.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { slot: 2, .. })), "{effects:?}");
    assert_eq!(d.active(), Some(0), "it comes forward when it reports");
    // a chosen one
    d.bar_request(Request::OpenLogin(Some(3)), now);
    assert!(d.take_effects().iter().any(|e| matches!(e, Effect::Spawn { slot: 3, .. })));
    // one that is logged in is not switched to by this button
    d.bar_request(Request::OpenLogin(Some(1)), now);
    assert_eq!(d.active(), Some(0));
    assert!(d.take_effects().is_empty());
    // everyone has a viewer: nothing to open
    let (mut full, now) = desk(2, 2);
    full.bar_request(Request::OpenLogin(None), now);
    assert!(full.take_effects().is_empty());
}

#[test]
fn the_bar_opens_the_panels_in_their_own_windows() {
    let (mut d, now) = desk(3, 3);
    d.bar_request(Request::Chat(true), now);
    assert!(d.panels().out(Panel::Chat) && d.bar_state(0, 0).chat_open);
    d.bar_request(Request::Chat(false), now);
    assert!(!d.panels().out(Panel::Chat));
    d.bar_request(Request::Inbox, now);
    assert!(d.panels().out(Panel::Inbox));
    // a monitor opens and closes; no panel has anything to do with it
    d.bar_request(Request::Monitor(1, true), now);
    assert!(d.slot(1).is_some_and(|s| s.monitor.is_some()));
    d.bar_request(Request::Monitor(1, false), now);
    assert!(d.slot(1).is_some_and(|s| s.monitor.is_none()));
    // all the characters behind the active one, never the one in front
    d.bar_request(Request::OpenMonitors, now);
    let open: Vec<usize> = d.slots().filter(|(_, s)| s.monitor.is_some()).map(|(i, _)| i).collect();
    assert_eq!(open, [1, 2]);
    // a monitor's own size and rate are kept when it is opened again
    let small = monitor::Spec {
        size: monitor::Size::Small,
        ..monitor::Spec::default()
    };
    d.set_monitor(1, Some(small));
    d.bar_request(Request::Monitor(1, true), now);
    d.bar_request(Request::OpenMonitors, now);
    assert_eq!(d.slot(1).and_then(|s| s.monitor), Some(small));
    // the cycling monitor, and the bar says it is open
    assert!(!d.bar_state(0, 0).cycling);
    d.bar_request(Request::Cycling(true), now);
    assert!(d.monitors().cycling.open && d.bar_state(0, 0).cycling);
    d.bar_request(Request::Cycling(false), now);
    assert!(!d.monitors().cycling.open);
    d.bar_request(Request::Cycling(true), now);
    // "Close all monitors": every character's, and the cycling one
    d.bar_request(Request::CloseMonitors, now);
    assert!(d.slots().all(|(_, s)| s.monitor.is_none()) && !d.monitors().cycling.open);
    // a monitor opened from the bar has the defaults the user chose
    d.edit_monitors(|monitors| monitors.defaults = small);
    d.bar_request(Request::Monitor(2, true), now);
    assert_eq!(d.slot(2).and_then(|s| s.monitor), Some(small));
    // an open monitor's entry can be clicked whatever its character does
    let state = d.bar_state(0, 0);
    assert!(state.character(2).is_some_and(|c| c.monitor && c.monitor_clickable()));
    assert!(state.character(0).is_some_and(|c| !c.watchable() && !c.monitor_clickable()));
    assert!(Request::Cycling(true).shows_a_window() && !Request::Cycling(false).shows_a_window());
    // a page of the host's window is the host's matter: the desk does nothing
    d.take_effects();
    d.bar_request(Request::Show(Page::Bar), now);
    assert!(d.take_effects().is_empty());
}

#[test]
fn the_bar_s_own_clicks_are_saved() {
    let (mut d, now) = desk(2, 2);
    d.bar_request(Request::Collapse(true), now);
    d.bar_request(Request::Pin(true), now);
    d.bar_request(Request::Dock(false), now);
    assert!(d.bar().collapsed && d.bar().pinned && d.bar().placement == Placement::Popped);
    d.bar_request(Request::Dock(true), now);
    assert_eq!(d.bar().placement, Placement::Docked);
    d.bar_request(Request::MuteBackground(true), now);
    d.bar_request(Request::VoiceFollows(false), now);
    assert!(d.mute_background() && !d.voice_follows());
}

#[test]
fn mute_background_cuts_every_background_character_and_keeps_each_one_s_choice() {
    let (mut d, now) = desk(3, 3);
    d.set_sound(1, BackgroundSound::All);
    let muted = |d: &Desk| [0, 1, 2].map(|slot| assigned(d, slot).sound_muted);
    assert_eq!(muted(&d), [false, false, true]);
    d.set_mute_background(true);
    assert_eq!(muted(&d), [false, true, true], "the active one is never cut");
    assert_eq!(d.slot(1).map(|s| s.sound), Some(BackgroundSound::All), "its own choice is kept");
    // the one that comes forward is heard, the one left is cut
    d.switch(1, now);
    assert_eq!(muted(&d), [true, false, true]);
    d.set_mute_background(false);
    assert_eq!(muted(&d), [true, false, true]);
    d.switch(0, now);
    assert_eq!(muted(&d), [false, false, true], "each one's own choice applies again");
}

#[test]
fn with_voice_following_the_active_character_has_it() {
    let (mut d, now) = desk(3, 3);
    assert_eq!((d.voice_holder(), voices(&d)), (Some(0), vec![0]));
    d.switch(2, now);
    settle(&mut d, now);
    assert_eq!((d.voice_holder(), voices(&d)), (Some(2), vec![2]));
    assert_eq!(d.voice(2), Voice::Allowed);
}

#[test]
fn with_voice_not_following_it_stays_where_it_is_and_is_heard_there() {
    let (mut d, now) = desk(3, 3);
    d.set_voice_follows(false);
    d.switch(1, now);
    settle(&mut d, now);
    assert_eq!(d.active(), Some(1));
    assert_eq!((d.voice_holder(), voices(&d)), (Some(0), vec![0]));
    assert_eq!(d.voice(0), Voice::Allowed);
    assert_eq!(d.voice(1), Voice::Off);
    assert_eq!(d.voice_waits_for(), None, "nobody waits: it is where it should be");
    // behind, the one that holds the voice is heard (a viewer that is cut
    // neither plays nor transmits it); the other background one is not
    assert!(!assigned(&d, 0).sound_muted);
    assert!(assigned(&d, 2).sound_muted);
    // "Mute background sound" cuts it too
    d.set_mute_background(true);
    assert!(assigned(&d, 0).sound_muted && assigned(&d, 0).voice_allowed);
    d.set_mute_background(false);
    // it stays through more switches
    d.switch(2, now);
    settle(&mut d, now);
    assert_eq!(voices(&d), vec![0]);
    // its viewer ends: the active one takes the voice, and keeps it
    let launch = d.slot(0).expect("slot").launch();
    d.exited(0, launch, true, now);
    settle(&mut d, now);
    assert_eq!((d.voice_holder(), voices(&d)), (Some(2), vec![2]));
    d.switch(1, now);
    settle(&mut d, now);
    assert_eq!(voices(&d), vec![2]);
}

#[test]
fn turning_voice_follows_on_again_gives_it_to_the_active_one_once_given_up() {
    let (mut d, now) = desk(2, 2);
    d.set_voice_follows(false);
    d.switch(1, now);
    settle(&mut d, now);
    assert_eq!(voices(&d), vec![0]);
    d.set_voice_follows(true);
    // revoked first: for a moment nobody is allowed, never two
    assert!(voices(&d).is_empty(), "{:?}", voices(&d));
    assert_eq!(d.voice_waits_for(), Some(0));
    settle(&mut d, now);
    assert_eq!(voices(&d), vec![1]);
    assert_eq!(d.voice(1), Voice::Allowed);
}

#[test]
fn never_two_voices_whatever_is_clicked() {
    let (mut d, now) = desk(4, 4);
    let mut most = 0;
    for step in 0..60usize {
        match step % 5 {
            0 => d.switch(step % 4, now),
            1 => d.set_voice_follows(step % 2 == 0),
            2 => d.bar_request(Request::Pick((step % 3) as u8), now),
            3 => d.set_mute_background(step % 3 == 0),
            _ => {
                // only some of them report: the others still hold what
                // they held
                report(&mut d, step % 4, now);
            }
        }
        most = most.max(voices(&d).len());
        assert!(voices(&d).len() <= 1, "step {step}: {:?}", voices(&d));
    }
    assert_eq!(most, 1);
}

#[test]
fn one_taskbar_button_asks_the_characters_behind_only_while_docked_and_someone_is_in_front() {
    let (mut d, now) = desk(3, 3);
    let off = |d: &Desk| [0, 1, 2].map(|slot| assigned(d, slot).off_taskbar);
    assert_eq!(off(&d), [false, true, true]);
    // switching swaps them
    d.switch(2, now);
    assert_eq!(off(&d), [true, true, false]);
    // popped out: every window is as usual
    d.bar_request(Request::Dock(false), now);
    assert_eq!(off(&d), [false; 3]);
    d.bar_request(Request::Dock(true), now);
    assert_eq!(off(&d), [true, true, false]);
    // the setting off
    assert!(d.edit_bar(|bar| bar.one_taskbar_button = false));
    assert_eq!(off(&d), [false; 3]);
    assert!(d.edit_bar(|bar| bar.one_taskbar_button = true));
    // the one in front ends: nobody is in front, so nobody is hidden (a
    // desk with no button at all could not be reached)
    let launch = d.slot(2).expect("slot").launch();
    d.exited(2, launch, true, now);
    assert_eq!(d.active(), None);
    assert!(!assigned(&d, 0).off_taskbar && !assigned(&d, 1).off_taskbar);
    d.switch(0, now);
    assert!(!assigned(&d, 0).off_taskbar && assigned(&d, 1).off_taskbar);
}
