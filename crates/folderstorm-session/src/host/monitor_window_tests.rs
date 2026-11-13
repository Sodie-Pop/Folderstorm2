//! Checkpoint 9's desk rules: each monitor has a window of its own, which
//! follows the character that is not in front; the cycling monitor asks
//! pictures of the one character it shows; what is remembered, and what a
//! settings file of before reads as. (In a file of its own, as checkpoint
//! 5's.)

use super::*;
use crate::monitor::{Rate, Size, Spec};
use crate::monitor_windows::Window;
use crate::panels::Place;

fn desk(count: usize) -> (Desk, Instant) {
    (Desk::new(Config::demo(count)), Instant::now())
}

/// A desk with `count` characters in the world, the first one in front.
fn running(count: usize) -> (Desk, Instant) {
    let (mut d, now) = desk(count);
    for slot in 0..count {
        d.launch(slot, slot == 0);
        d.connected(slot, 1, now);
        let status = Status {
            screen: Screen::World,
            ..Default::default()
        };
        d.status(slot, 1, status, now);
    }
    d.take_effects();
    (d, now)
}

fn role(d: &Desk, slot: usize) -> Role {
    d.slot(slot).expect("slot").sent.expect("sent").0.role
}

fn open(d: &Desk) -> Vec<usize> {
    d.slots().filter(|(_, s)| s.monitor.is_some()).map(|(i, _)| i).collect()
}

fn saves(d: &mut Desk) -> usize {
    d.take_effects().iter().filter(|e| matches!(e, Effect::Save)).count()
}

fn place(x: i32, y: i32) -> Place {
    Place {
        position: Some([x, y]),
        size: [480, 300],
    }
}

#[test]
fn switch_swaps_the_monitor_to_the_character_that_was_in_front() {
    let (mut d, now) = running(3);
    let small = Spec {
        size: Size::Small,
        rate: Rate::Two,
    };
    d.set_monitor(1, Some(small));
    assert!(d.edit_monitor_window(1, |w| {
        w.placed(place(900, 40));
        w.pinned = true;
        w.chat = true;
    }));
    // (what 0 remembers of a window it had once)
    assert!(d.edit_monitor_window(0, |w| {
        w.placed(place(10, 10));
    }));
    d.monitors_seen([false, true, false, false, false], now);
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
    d.take_effects();
    // the monitor's "Switch": 1 comes to the front, 0 is watched in its
    // place, in the same window
    d.switch(1, now);
    assert_eq!(d.active(), Some(1));
    assert_eq!(open(&d), [0]);
    assert_eq!(d.take_monitor_moves(), [(1, 0)]);
    assert!(d.take_monitor_moves().is_empty(), "told once");
    let watched = d.slot(0).expect("slot");
    assert_eq!(watched.monitor, Some(small), "its size and rate");
    assert_eq!(
        watched.monitor_window,
        Window {
            place: Some(place(900, 40)),
            pinned: true,
            chat: true
        }
    );
    // the two swap: what 0 remembered is 1's now
    assert_eq!(d.slot(1).expect("slot").monitor_window.place, Some(place(10, 10)));
    assert!(saves(&mut d) >= 1);
    // the one that left is asked for pictures at once (the window is the
    // same: it is on screen until it says otherwise)
    assert_eq!(role(&d, 1), Role::Active);
    assert!(matches!(role(&d, 0), Role::Monitor(order) if order.size == Size::Small));
    assert!(matches!(d.watch(0), Watch::Asked { .. }));
    assert_eq!(d.watch(1), Watch::Closed);
    // and back
    d.switch(0, now);
    assert_eq!(open(&d), [1]);
    assert_eq!(d.take_monitor_moves(), [(0, 1)]);
    assert_eq!(d.slot(1).expect("slot").monitor_window.place, Some(place(900, 40)));
    assert_eq!(d.slot(0).expect("slot").monitor_window.place, Some(place(10, 10)));
}

#[test]
fn the_monitor_follows_whatever_makes_the_switch() {
    let (mut d, now) = running(3);
    d.set_monitor(2, Some(Spec::default()));
    // a hotkey
    assert_eq!(d.hotkey(hotkeys::Action::Character(2), now), Some(2));
    assert_eq!((d.active(), open(&d)), (Some(2), vec![0]));
    // the bar's list
    d.bar_request(bar::Request::Pick(0), now);
    assert_eq!((d.active(), open(&d)), (Some(0), vec![2]));
    // a click in the character's own window
    let launch = d.slot(2).expect("slot").launch();
    let status = Status {
        assign_seq: d.slot(2).expect("slot").sent.expect("sent").0.seq,
        screen: Screen::World,
        focused: true,
        ..Default::default()
    };
    d.status(2, launch, status, now);
    assert_eq!((d.active(), open(&d)), (Some(2), vec![0]));
    assert_eq!(d.take_monitor_moves(), [(2, 0), (0, 2), (2, 0)]);
}

#[test]
fn a_monitor_stays_where_nothing_can_take_its_place() {
    let (mut d, now) = running(3);
    d.set_monitor(1, Some(Spec::default()));
    // a character without a monitor comes to the front: nothing moves
    d.switch(2, now);
    assert_eq!(open(&d), [1]);
    // the one that leaves the front has its own monitor: both keep theirs,
    // and the one in front says so
    d.set_monitor(2, Some(Spec::default()));
    d.switch(1, now);
    assert_eq!(open(&d), [1, 2]);
    assert_eq!(d.watch(1), Watch::InFront);
    assert!(d.take_monitor_moves().is_empty());
    // nobody was in front (the first character started with its monitor
    // open): it keeps it
    let (mut d, now) = desk(2);
    d.set_monitor(0, Some(Spec::default()));
    d.launch(0, true);
    d.connected(0, 1, now);
    assert_eq!((d.active(), open(&d)), (Some(0), vec![0]));
    assert!(d.take_monitor_moves().is_empty());
    // the one that was in front has no viewer any more: nothing to watch
    let (mut d, now) = running(2);
    d.set_monitor(1, Some(Spec::default()));
    d.exited(0, 1, true, now);
    d.switch(1, now);
    assert_eq!(open(&d), [1]);
}

#[test]
fn every_background_character_stays_watched_through_any_switch() {
    let (mut d, now) = running(5);
    d.bar_request(bar::Request::OpenMonitors, now);
    assert_eq!(open(&d), [1, 2, 3, 4]);
    d.monitors_seen([true; MAX_SLOTS], now);
    for to in [3, 1, 4, 0, 2] {
        d.switch(to, now);
        let watched = open(&d);
        assert_eq!(watched.len(), 4);
        assert!(!watched.contains(&to));
        // never more than four are asked, and never the one in front
        let asked = (0..5).filter(|slot| matches!(role(&d, *slot), Role::Monitor(_))).count();
        assert_eq!(asked, 4);
        assert_eq!(role(&d, to), Role::Active);
    }
}

#[test]
fn a_window_s_place_pin_and_chat_are_saved_with_its_character() {
    let (mut d, _) = desk(2);
    d.take_effects();
    assert!(d.edit_monitor_window(1, |w| w.pinned = true));
    assert!(!d.edit_monitor_window(1, |w| w.pinned = true), "as it was");
    assert!(d.edit_monitor_window(1, |w| {
        w.placed(place(300, 200));
    }));
    assert!(!d.edit_monitor_window(MAX_SLOTS, |w| w.chat = true));
    // saved, and nothing is said to any viewer
    let effects = d.take_effects();
    assert_eq!(effects.len(), 2);
    assert!(effects.iter().all(|e| matches!(e, Effect::Save)), "{effects:?}");
    let config = d.config();
    assert!(config.slots[0].monitor_window.is_default());
    assert_eq!(
        config.slots[1].monitor_window,
        Window {
            place: Some(place(300, 200)),
            pinned: true,
            chat: false
        }
    );
    // written and read back; a character without a window writes nothing
    let text = serde_json::to_string(&config).expect("written");
    assert_eq!(text.matches("monitor_window").count(), 1);
    let next = Desk::new(serde_json::from_str(&text).expect("read"));
    assert_eq!(next.slot(1).expect("slot").monitor_window, config.slots[1].monitor_window);
    // a place no window can have is dropped when the file is read
    let bad = text.replace("[480,300]", "[1,1]");
    let next = Desk::new(serde_json::from_str(&bad).expect("read"));
    let window = next.slot(1).expect("slot").monitor_window;
    assert!(window.place.is_none() && window.pinned);
}

#[test]
fn a_settings_file_of_before_reads_with_its_monitors_open() {
    // a file of checkpoint 8: two characters, the second one's monitor
    // open, the monitors' panel out with its place
    let old = r#"{
        "slots": [
            {"slot": 0, "profile": "Main2", "standby": "Warm", "sound": "Muted", "monitor": null, "colour": "None"},
            {"slot": 1, "profile": "Alt1", "standby": "Economy", "sound": "Muted",
             "monitor": {"size": "Small", "rate": "Two"}, "colour": "Teal", "alias": "Shop"}
        ],
        "stage": true,
        "chat_sound": false,
        "panels": {
            "chat": {"out": true, "place": {"position": [100, 50], "size": [640, 480]}},
            "controls": {"out": false, "place": null},
            "monitors": {"out": true, "place": {"position": [900, 40], "size": [700, 520]}},
            "inbox": {"out": false, "place": null}
        },
        "condensed": true,
        "mute_background": false,
        "voice_follows": true
    }"#;
    let config: Config = serde_json::from_str(old).expect("reads");
    let d = Desk::new(config);
    // its monitor is open, as chosen, in a window of its own at its
    // first size; the rest is as it was
    let alt = d.slot(1).expect("slot");
    assert_eq!(
        alt.monitor,
        Some(Spec {
            size: Size::Small,
            rate: Rate::Two
        })
    );
    assert!(alt.monitor_window.is_default());
    assert_eq!(alt.alias.as_deref(), Some("Shop"));
    assert!(d.slot(0).expect("slot").monitor.is_none());
    assert!(d.panels().out(panels::Panel::Chat) && d.condensed());
    // the cycling monitor is closed, with its first settings
    assert_eq!(*d.monitors(), monitor_windows::Config::default());
    assert!(!d.monitors().cycling.open && d.monitors().cycling.pause_typing);
    assert_eq!(d.monitors().cycling.interval_s, 5);
    // what is written has the new settings and nothing of the old window
    let written = serde_json::to_string(&d.config()).expect("written");
    assert!(written.contains("\"monitors\":{\"defaults\""), "{written}");
    assert!(!written.contains("\"monitors\":{\"out\""), "{written}");
    assert_eq!(Desk::new(serde_json::from_str(&written).expect("read")).config(), d.config());
}

#[test]
fn the_monitors_settings_are_saved_and_checked() {
    let (mut d, _) = desk(1);
    d.take_effects();
    let large = Spec {
        size: Size::Large,
        rate: Rate::Half,
    };
    assert!(d.edit_monitors(|m| {
        m.defaults = large;
        m.cycling.interval_s = 30;
        m.cycling.pause_typing = false;
    }));
    assert_eq!(saves(&mut d), 1);
    assert!(!d.edit_monitors(|m| m.defaults = large), "as it was: nothing to save");
    assert_eq!(saves(&mut d), 0);
    // a turn no window offers is brought within the bounds
    d.edit_monitors(|m| m.cycling.interval_s = 0);
    assert_eq!(d.monitors().cycling.interval_s, monitor_windows::MIN_INTERVAL_S);
    let next = Desk::new(d.config());
    assert_eq!(next.monitors().defaults, large);
    assert!(!next.monitors().cycling.pause_typing);
    // a monitor opens with the defaults
    d.open_monitor(0);
    assert_eq!(d.slot(0).expect("slot").monitor, Some(large));
    // one that is open keeps its own
    d.edit_monitors(|m| m.defaults = Spec::default());
    d.open_monitor(0);
    assert_eq!(d.slot(0).expect("slot").monitor, Some(large));
}

#[test]
fn the_cycling_monitor_asks_only_the_character_it_shows() {
    let (mut d, now) = running(4);
    assert_eq!(d.cycling_candidates(), [1, 2, 3]);
    // closed: nothing is asked, whatever a window says it shows
    d.cycling_seen(Some(1), now);
    assert_eq!(role(&d, 1), Role::Background);
    assert_eq!(d.cycling_watch(1), Watch::Closed);
    d.set_cycling(true);
    assert_eq!(d.cycling_watch(1), Watch::Hidden);
    assert_eq!(role(&d, 1), Role::Background, "not on screen yet");
    // it shows 1: only 1 is asked
    d.cycling_seen(Some(1), now);
    assert!(matches!(d.cycling_watch(1), Watch::Asked { lowered: false, .. }));
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
    assert_eq!((role(&d, 2), role(&d, 3)), (Role::Background, Role::Background));
    // its turn passes: 2 is asked and 1 is a background character again
    d.cycling_seen(Some(2), now);
    assert_eq!(role(&d, 1), Role::Background);
    assert!(matches!(role(&d, 2), Role::Monitor(_)));
    assert_eq!(d.cycling_watch(1), Watch::Hidden);
    // its window is covered or minimised: nobody is asked
    d.cycling_seen(None, now);
    assert_eq!(role(&d, 2), Role::Background);
    // the character in front is never asked, shown or not
    d.cycling_seen(Some(0), now);
    assert_eq!(role(&d, 0), Role::Active);
    assert_eq!(d.cycling_watch(0), Watch::InFront);
    // closed: nobody is asked any more
    d.cycling_seen(Some(3), now);
    assert!(matches!(role(&d, 3), Role::Monitor(_)));
    d.set_cycling(false);
    assert_eq!(role(&d, 3), Role::Background);
    assert_eq!(d.cycling_watch(3), Watch::Closed);
    // the candidates: in the world, behind the active one, not closing
    d.switch(2, now);
    assert_eq!(d.cycling_candidates(), [0, 1, 3]);
    d.close(3);
    assert_eq!(d.cycling_candidates(), [0, 1]);
    let login = Status {
        screen: Screen::Login,
        ..Default::default()
    };
    d.status(1, 1, login, now);
    assert_eq!(d.cycling_candidates(), [0]);
}

#[test]
fn a_character_shown_twice_is_asked_once_for_the_larger_and_the_faster() {
    let (mut d, now) = running(3);
    let own = Spec {
        size: Size::Small,
        rate: Rate::Two,
    };
    d.set_monitor(1, Some(own));
    d.monitors_seen([false, true, false, false, false], now);
    assert!(matches!(role(&d, 1), Role::Monitor(order) if order.size == Size::Small && order.interval_ms == 500));
    d.edit_monitors(|m| {
        m.cycling.open = true;
        m.cycling.spec = Spec {
            size: Size::Large,
            rate: Rate::Half,
        };
    });
    d.cycling_seen(Some(1), now);
    assert!(matches!(role(&d, 1), Role::Monitor(order) if order.size == Size::Large && order.interval_ms == 500));
    // one order, one viewer asked: the budget counts it once
    let asked = (0..3).filter(|slot| matches!(role(&d, *slot), Role::Monitor(_))).count();
    assert_eq!(asked, 1);
    // the cycling monitor moves on: its own monitor's order again
    d.cycling_seen(Some(2), now);
    assert!(matches!(role(&d, 1), Role::Monitor(order) if order.size == Size::Small));
    assert!(matches!(role(&d, 2), Role::Monitor(order) if order.size == Size::Large && order.interval_ms == 2000));
    // the cycling monitor's pictures are within the same budget
    d.edit_monitors(|m| {
        m.cycling.spec = Spec {
            size: Size::Large,
            rate: Rate::Five,
        };
    });
    d.set_monitor(
        1,
        Some(Spec {
            size: Size::Large,
            rate: Rate::Five,
        }),
    );
    assert!(matches!(d.cycling_watch(2), Watch::Asked { lowered: true, .. }));
    assert!(matches!(d.watch(1), Watch::Asked { lowered: true, .. }));
}

#[test]
fn nobody_is_minimised_behind_while_the_cycling_monitor_is_open() {
    let (mut d, now) = running(3);
    for slot in 0..3 {
        d.set_standby(slot, Standby::Economy);
    }
    d.set_cycling(true);
    d.take_effects();
    d.switch(1, now);
    let minimised = d.take_effects().iter().any(|e| {
        matches!(
            e,
            Effect::Send {
                message: ToViewer::Minimise,
                ..
            }
        )
    });
    assert!(!minimised);
    assert_eq!(d.slot(0).expect("slot").sent.expect("sent").0.standby, Standby::Warm);
    // the choice is kept: closed, the one left behind is Economy again
    d.set_cycling(false);
    assert_eq!(d.slot(0).expect("slot").sent.expect("sent").0.standby, Standby::Economy);
    assert_eq!(d.slot(0).expect("slot").standby, Standby::Economy);
}
