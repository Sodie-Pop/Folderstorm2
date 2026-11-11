//! Checkpoint 4's desk rules: quick actions, hotkeys, next and previous,
//! what is heard in the background, and who holds the voice. (In a file of
//! its own: the earlier checkpoints' tests are in `host.rs`.)

use super::*;
use crate::hotkeys::Action;

fn desk(count: usize) -> (Desk, Instant) {
    (Desk::new(Config::demo(count)), Instant::now())
}

fn start(d: &mut Desk, slot: usize, activate: bool, now: Instant) -> u32 {
    d.launch(slot, activate);
    let launch = d.slot(slot).expect("slot").launch();
    d.connected(slot, launch, now);
    launch
}

/// The viewer's report that it holds, and follows, the latest assignment
/// sent to it: its voice is off when the assignment says so.
fn report(d: &mut Desk, slot: usize, screen: Screen, now: Instant) {
    let s = d.slot(slot).expect("slot");
    let (launch, sent) = (s.launch(), s.sent.expect("sent").0);
    let status = Status {
        assign_seq: sent.seq,
        screen,
        sound_muted: sent.sound_muted,
        voice_allowed: sent.voice_allowed,
        ..s.status.clone().unwrap_or_default()
    };
    d.status(slot, launch, status, now);
}

/// Every running viewer reports, until nothing changes any more.
fn settle(d: &mut Desk, now: Instant) {
    for _ in 0..4 {
        let running: Vec<usize> = d.slots().filter(|(_, s)| s.phase == Phase::Running).map(|(i, _)| i).collect();
        for slot in running {
            report(d, slot, Screen::World, now);
        }
    }
}

fn assigned(d: &Desk, slot: usize) -> Assignment {
    d.slot(slot).expect("slot").sent.expect("sent").0
}

fn spawns(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Spawn { .. })).count()
}

fn shown(effects: &[Effect]) -> Vec<usize> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                slot,
                message: ToViewer::Show { .. },
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect()
}

/// The slots whose viewers are told, as things stand, that they may use
/// voice.
fn voices(d: &Desk) -> Vec<usize> {
    d.slots()
        .filter(|(_, s)| s.phase == Phase::Running && s.sent.is_some_and(|(a, _)| a.voice_allowed))
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn the_quick_action_follows_where_the_viewer_is() {
    let (mut d, now) = desk(3);
    // the decision table, a state after the other
    assert_eq!(d.quick(0), Quick::Launch, "never started");
    assert_eq!(d.quick(4), Quick::Nothing, "an empty slot");
    assert_eq!(d.quick(99), Quick::Nothing, "no such slot");
    d.launch(0, false);
    assert_eq!(d.quick(0), Quick::Wait, "starting");
    d.connected(0, 1, now);
    assert_eq!(d.quick(0), Quick::Show, "the active character, before its first report");
    report(&mut d, 0, Screen::Login, now);
    assert_eq!(d.quick(0), Quick::LogIn, "at its login screen");
    report(&mut d, 0, Screen::Loading, now);
    assert_eq!(d.quick(0), Quick::Show);
    report(&mut d, 0, Screen::World, now);
    assert_eq!(d.quick(0), Quick::Show, "active, in world");
    start(&mut d, 1, false, now);
    report(&mut d, 1, Screen::World, now);
    assert_eq!(d.quick(1), Quick::Switch, "running behind the active one");
    report(&mut d, 1, Screen::Login, now);
    assert_eq!(d.quick(1), Quick::LogIn, "behind, at its login screen");
    d.close(1);
    assert_eq!(d.quick(1), Quick::Nothing, "closing");
    d.exited(1, 1, true, now);
    assert_eq!(d.quick(1), Quick::Restart, "closed");
    d.launch(2, false);
    d.spawn_failed(2, 1);
    assert_eq!(d.quick(2), Quick::Launch, "could not start: try again");
    // a viewer the host let go still runs: no second process for its slot
    let (mut u, _) = desk(1);
    u.launch(0, true);
    u.refused(0, 1, Refusal::Token);
    u.take_effects();
    assert_eq!(u.quick(0), Quick::Nothing, "unmanaged");
    assert_eq!(u.quick_act(0, now), Quick::Nothing);
    assert_eq!(u.hotkey(Action::Character(0), now), None);
    assert!(u.take_effects().is_empty());
    // "Quit all": nothing is started, the closing ones are left alone
    d.quit_all(now);
    assert_eq!(d.quick(0), Quick::Nothing);
    assert_eq!(d.quick(1), Quick::Nothing);
    assert_eq!(d.quick(2), Quick::Nothing);
}

#[test]
fn a_quick_action_never_starts_a_second_process_for_a_slot() {
    let (mut d, now) = desk(2);
    assert_eq!(d.quick_act(0, now), Quick::Launch);
    assert_eq!(spawns(&d.take_effects()), 1);
    // pressed again and again while it starts, runs, and closes
    for _ in 0..3 {
        assert_eq!(d.quick_act(0, now), Quick::Wait);
    }
    assert_eq!(spawns(&d.take_effects()), 0);
    d.connected(0, 1, now);
    report(&mut d, 0, Screen::Login, now);
    for _ in 0..3 {
        assert_eq!(d.quick_act(0, now), Quick::LogIn);
    }
    report(&mut d, 0, Screen::World, now);
    assert_eq!(d.quick_act(0, now), Quick::Show);
    d.close(0);
    assert_eq!(d.quick_act(0, now), Quick::Nothing);
    let effects = d.take_effects();
    assert_eq!(spawns(&effects), 0);
    assert_eq!(d.slot(0).expect("slot").launch(), 1);
    // once its process has ended, and only then, a new one
    d.exited(0, 1, true, now);
    assert_eq!(d.quick_act(0, now), Quick::Restart);
    assert_eq!(d.take_effects(), vec![Effect::Spawn { slot: 0, launch: 2 }]);
    assert_eq!(d.quick_act(0, now), Quick::Wait);
    assert_eq!(spawns(&d.take_effects()), 0);
}

#[test]
fn a_quick_action_at_the_login_screen_brings_that_window_forward() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    report(&mut d, 0, Screen::World, now);
    report(&mut d, 1, Screen::Login, now);
    d.take_effects();
    assert_eq!(d.quick_act(1, now), Quick::LogIn);
    let effects = d.take_effects();
    assert!(effects.contains(&Effect::AllowForeground { slot: 1 }));
    assert_eq!(shown(&effects), [1]);
    assert_eq!(d.active(), Some(1), "the user logs in there: it is the active one");
    // a starting viewer comes forward when it reports, whatever was asked
    // of it at launch
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    d.launch(1, false);
    assert_eq!(d.quick_act(1, now), Quick::Wait);
    d.take_effects();
    d.connected(1, 1, now);
    assert_eq!(d.active(), Some(1));
    assert_eq!(shown(&d.take_effects()), [1]);
}

#[test]
fn a_character_s_hotkey_is_its_quick_action_with_the_right_to_the_foreground() {
    let (mut d, now) = desk(3);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    settle(&mut d, now);
    d.take_effects();
    // while a viewer has the keyboard (the host's window has not): the
    // viewer is given the foreground, then asked to show
    assert_eq!(d.hotkey(Action::Character(1), now), Some(1));
    assert_eq!(d.active(), Some(1));
    let effects = d.take_effects();
    let allow = effects.iter().position(|e| *e == Effect::AllowForeground { slot: 1 });
    let show = effects.iter().position(|e| {
        matches!(
            e,
            Effect::Send {
                slot: 1,
                message: ToViewer::Show { .. },
                ..
            }
        )
    });
    assert!(allow.is_some() && allow < show, "{effects:?}");
    // a stopped character's hotkey starts it; an empty slot's does nothing
    assert_eq!(d.hotkey(Action::Character(2), now), Some(2));
    assert_eq!(spawns(&d.take_effects()), 1);
    assert_eq!(d.hotkey(Action::Character(2), now), Some(2), "starting: it waits");
    assert_eq!(spawns(&d.take_effects()), 0);
    assert_eq!(d.hotkey(Action::Character(4), now), None);
    assert!(d.take_effects().is_empty());
}

#[test]
fn next_and_previous_go_round_the_characters_that_run() {
    let (mut d, now) = desk(5);
    // nothing runs: nowhere to go, and nothing is started
    assert_eq!(d.neighbour(true), None);
    assert_eq!(d.hotkey(Action::Next, now), None);
    assert!(d.take_effects().is_empty());
    // slots 0, 2 and 4 run; 1 is stopped, 3 is starting
    start(&mut d, 0, true, now);
    start(&mut d, 2, false, now);
    start(&mut d, 4, false, now);
    d.launch(3, false);
    settle(&mut d, now);
    d.take_effects();
    let mut went = Vec::new();
    for _ in 0..4 {
        went.push(d.hotkey(Action::Next, now));
        settle(&mut d, now);
    }
    assert_eq!(went, [Some(2), Some(4), Some(0), Some(2)]);
    let mut went = Vec::new();
    for _ in 0..4 {
        went.push(d.hotkey(Action::Previous, now));
        settle(&mut d, now);
    }
    assert_eq!(went, [Some(0), Some(4), Some(2), Some(0)]);
    assert_eq!(spawns(&d.take_effects()), 0, "next and previous never start a viewer");
    // a closing character is passed over
    d.close(2);
    assert_eq!(d.neighbour(true), Some(4));
    assert_eq!(d.neighbour(false), Some(4));
    // alone: nowhere to go
    d.close(4);
    assert_eq!(d.neighbour(true), None);
    assert_eq!(d.hotkey(Action::Previous, now), None);
    // no active character (it ended): the first, or the last
    let (mut d, now) = desk(3);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    start(&mut d, 2, false, now);
    d.exited(0, 1, true, now);
    assert_eq!(d.active(), None);
    assert_eq!(d.neighbour(true), Some(1));
    assert_eq!(d.neighbour(false), Some(2));
}

#[test]
fn hotkeys_never_give_the_voice_to_two_characters() {
    let (mut d, now) = desk(3);
    for slot in 0..3 {
        start(&mut d, slot, slot == 0, now);
    }
    settle(&mut d, now);
    assert_eq!(voices(&d), [0]);
    assert_eq!(d.voice(0), Voice::Allowed);
    assert_eq!((d.voice(1), d.voice(2)), (Voice::Off, Voice::Off));
    // pressed faster than the viewers answer: whoever is active waits for
    // every character that was allowed voice and has not given it up
    let presses = [
        Action::Next,
        Action::Next,
        Action::Character(0),
        Action::Previous,
        Action::Character(1),
        Action::Next,
    ];
    for (i, action) in presses.into_iter().enumerate() {
        d.hotkey(action, now);
        assert!(voices(&d).len() <= 1, "press {i}: {:?}", voices(&d));
        let active = d.active().expect("active");
        for slot in 0..3 {
            match d.voice(slot) {
                Voice::Allowed => assert_eq!(slot, active),
                Voice::WaitsFor(other) => {
                    assert_eq!(slot, active);
                    assert_ne!(other, active);
                    assert_eq!(d.voice(other), Voice::GivingUp);
                    assert!(!assigned(&d, slot).voice_allowed);
                }
                Voice::GivingUp => assert_ne!(slot, active),
                Voice::Off => {}
            }
        }
        if i % 2 == 1 {
            // now and then one viewer answers, not the others
            report(&mut d, i % 3, Screen::World, now);
            assert!(voices(&d).len() <= 1, "after a report at press {i}");
        }
    }
    // everyone has answered: the active character, and it alone
    settle(&mut d, now);
    let active = d.active().expect("active");
    assert_eq!(voices(&d), [active]);
    assert_eq!(d.voice(active), Voice::Allowed);
    assert_eq!(d.voice_waits_for(), None);
    for slot in (0..3).filter(|s| *s != active) {
        assert_eq!(d.voice(slot), Voice::Off);
        let a = assigned(&d, slot);
        assert_eq!((a.role, a.voice_allowed), (Role::Background, false));
    }
}

#[test]
fn the_voice_shows_as_allowed_only_once_the_viewer_says_so() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    // told, not answered yet
    assert!(assigned(&d, 0).voice_allowed);
    assert_eq!(d.voice(0), Voice::Off);
    settle(&mut d, now);
    assert_eq!(d.voice(0), Voice::Allowed);
    d.switch(1, now);
    assert_eq!(d.voice(1), Voice::WaitsFor(0));
    assert_eq!(d.voice(0), Voice::GivingUp);
    // the old one ends instead of answering: nothing transmits any more
    d.exited(0, 1, false, now);
    assert_eq!(d.voice(0), Voice::Off);
    assert_eq!(d.voice(1), Voice::Off, "told, not answered yet");
    settle(&mut d, now);
    assert_eq!(d.voice(1), Voice::Allowed);
    assert_eq!(d.voice(4), Voice::Off, "an empty slot");
}

#[test]
fn a_character_heard_in_the_background_keeps_its_sound_and_never_the_voice() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    settle(&mut d, now);
    d.take_effects();
    assert!(assigned(&d, 1).sound_muted, "cut by default");
    d.set_sound(1, BackgroundSound::All);
    let b = assigned(&d, 1);
    assert_eq!((b.role, b.sound_muted, b.voice_allowed), (Role::Background, false, false));
    let effects = d.take_effects();
    assert!(effects.contains(&Effect::Save));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send {
            slot: 1,
            message: ToViewer::Assign(a),
            ..
        } if !a.sound_muted
    )));
    // the same choice again changes nothing
    d.set_sound(1, BackgroundSound::All);
    assert!(d.take_effects().is_empty());
    // it is saved, read back, and kept across a restart of its viewer
    let saved = d.config();
    assert_eq!(saved.slots[1].sound, BackgroundSound::All);
    assert_eq!(saved.slots[0].sound, BackgroundSound::Muted);
    let text = serde_json::to_string(&saved).expect("written");
    let read: Config = serde_json::from_str(&text).expect("read");
    assert_eq!(Desk::new(read).slot(1).expect("slot").sound, BackgroundSound::All);
    d.exited(1, 1, true, now);
    start(&mut d, 1, false, now);
    assert!(!assigned(&d, 1).sound_muted);
    // the active character is heard whatever its choice
    d.set_sound(1, BackgroundSound::Muted);
    d.switch(1, now);
    assert!(!assigned(&d, 1).sound_muted);
    assert!(assigned(&d, 0).sound_muted);
}

#[test]
fn the_hotkeys_are_part_of_the_settings_and_saved_when_they_change() {
    let (mut d, _) = desk(1);
    assert_eq!(d.hotkeys(), &hotkeys::Config::default());
    assert!(d.set_hotkey(Action::Next, "ctrl+shift+f8"));
    assert_eq!(d.take_effects(), vec![Effect::Save]);
    assert!(!d.set_hotkey(Action::Next, "Ctrl+Shift+F8"));
    assert!(d.take_effects().is_empty());
    assert_eq!(d.config().hotkeys.next, "Ctrl+Shift+F8");
    let off = hotkeys::Config {
        enabled: false,
        ..d.hotkeys().clone()
    };
    assert!(d.set_hotkeys(off.clone()));
    assert!(!d.set_hotkeys(off));
    assert_eq!(d.take_effects(), vec![Effect::Save]);
    assert!(d.hotkeys().bindings().is_empty());
    // a settings file from before the hotkeys reads with the defaults,
    // and what it holds of them is cleaned
    let read: Config = serde_json::from_str(r#"{"slots":[{"slot":0,"profile":"a"}]}"#).expect("read");
    assert_eq!(read.hotkeys, hotkeys::Config::default());
    assert_eq!(read.slots[0].sound, BackgroundSound::Muted);
    let read: Config = serde_json::from_str(r#"{"hotkeys":{"characters":["Ctrl+Alt+9"]}}"#).expect("read");
    let d = Desk::new(read);
    assert_eq!(d.hotkeys().characters.len(), MAX_SLOTS);
    assert_eq!(d.hotkeys().text(Action::Character(0)), "Ctrl+Alt+9");
    assert_eq!(d.hotkeys().text(Action::Character(1)), "");
}
