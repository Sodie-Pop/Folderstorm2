//! The desk's side of the video memory plan (`crate::memory`): each
//! character's share in its assignment, Economy's release, a watched
//! Economy character kept behind with a small share, the monitor swap
//! kept, and the user's background share saved and said at once.

use super::*;
use crate::ipc::{Memory, Release};
use crate::monitor::Spec;

fn desk(count: usize) -> (Desk, Instant) {
    (Desk::new(Config::demo(count)), Instant::now())
}

fn start(d: &mut Desk, slot: usize, activate: bool, now: Instant) -> u32 {
    d.launch(slot, activate);
    let launch = d.slot(slot).expect("slot").launch();
    d.connected(slot, launch, now);
    launch
}

fn memory(d: &Desk, slot: usize) -> Memory {
    d.slot(slot).expect("slot").sent.expect("sent").0.memory
}

fn report(d: &mut Desk, slot: usize, vram_mb: u32, now: Instant) {
    let launch = d.slot(slot).expect("slot").launch();
    let status = Status {
        screen: Screen::World,
        vram_mb,
        assign_seq: d.slot(slot).expect("slot").sent.map_or(0, |(a, _)| a.seq),
        ..Default::default()
    };
    d.status(slot, launch, status, now);
}

fn minimised(effects: &[Effect], slot: usize) -> bool {
    effects.iter().any(|e| {
        matches!(e, Effect::Send {
            slot: s,
            message: ToViewer::Minimise,
            ..
        } if *s == slot)
    })
}

#[test]
fn every_character_behind_holds_the_background_share_and_the_active_one_the_rest() {
    let (mut d, now) = desk(3);
    for slot in 0..3 {
        start(&mut d, slot, slot == 0, now);
    }
    assert_eq!(d.background_memory_mb(), memory::DEFAULT_BACKGROUND_MB);
    for slot in 1..3 {
        assert_eq!(
            memory(&d, slot),
            Memory {
                share_mb: memory::DEFAULT_BACKGROUND_MB,
                reserved_mb: 0,
                release: Release::Keep,
            },
            "warm"
        );
    }
    // nothing said yet of what they hold: their whole shares are left
    assert_eq!(memory(&d, 0).share_mb, 0);
    assert_eq!(memory(&d, 0).reserved_mb, 2 * memory::DEFAULT_BACKGROUND_MB);
    // they hold 1.5 GB and 300 MB: only the second's room is left
    report(&mut d, 1, 1500, now);
    report(&mut d, 2, 300, now);
    assert_eq!(memory(&d, 0).reserved_mb, 768, "724 MB, in steps of 256");
}

#[test]
fn economy_releases_everything_and_is_minimised_behind() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    d.take_effects();
    d.set_standby(1, Standby::Economy);
    // the release is said at once; the window waits for the next switch
    assert_eq!(memory(&d, 1).release, Release::Full);
    assert!(!minimised(&d.take_effects(), 1));
    // Economy holds what it holds, already outside the active one's budget
    report(&mut d, 1, 200, now);
    assert_eq!(memory(&d, 0).reserved_mb, 0);
    // a switch: the one that leaves releases, and is minimised after it
    d.set_standby(0, Standby::Economy);
    d.switch(1, now);
    let effects = d.take_effects();
    assert!(minimised(&effects, 0));
    assert_eq!(memory(&d, 0).release, Release::Full);
    // (the assignment comes before the minimise: the viewer knows to release)
    let at = |wanted: fn(&ToViewer) -> bool| {
        effects
            .iter()
            .position(|e| matches!(e, Effect::Send { slot: 0, message, .. } if wanted(message)))
    };
    assert!(at(|m| matches!(m, ToViewer::Assign(_))) < at(|m| matches!(m, ToViewer::Minimise)));
    assert_eq!(memory(&d, 1).release, Release::Keep);
    assert_eq!(memory(&d, 1).share_mb, 0, "the active one has no share");
}

#[test]
fn a_watched_economy_character_stays_behind_with_a_small_share() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    d.set_standby(1, Standby::Economy);
    d.set_monitor(1, Some(Spec::default()));
    // open, but not on screen yet: nobody watches it
    assert_eq!(memory(&d, 1).release, Release::Full);
    d.monitors_seen([true; MAX_SLOTS], now);
    let watched = memory(&d, 1);
    assert_eq!(watched.release, Release::Lowered);
    assert_eq!(watched.share_mb, memory::WATCHED_ECONOMY_MB);
    // its window stays behind for the pictures
    assert_eq!(d.slot(1).expect("slot").sent.expect("sent").0.standby, Standby::Warm);
    // its room counts for the active one
    report(&mut d, 1, 100, now);
    assert_eq!(memory(&d, 0).reserved_mb, 512);
    // closed: Economy again
    d.set_monitor(1, None);
    assert_eq!(memory(&d, 1).release, Release::Full);
    // the cycling monitor on screen keeps every Economy character behind too
    d.edit_monitors(|config| config.cycling.open = true);
    assert_eq!(memory(&d, 1).release, Release::Full, "open, not on screen");
    d.cycling_seen(Some(1), now);
    assert_eq!(memory(&d, 1).release, Release::Lowered);
}

#[test]
fn the_monitor_swap_stays_and_the_character_that_left_is_watched_with_little() {
    let (mut d, now) = desk(2);
    for slot in 0..2 {
        start(&mut d, slot, slot == 0, now);
        d.set_standby(slot, Standby::Economy);
    }
    // the second character's monitor is open and on screen; it comes to
    // the front
    d.set_monitor(1, Some(Spec::default()));
    d.monitors_seen([true; MAX_SLOTS], now);
    d.switch(1, now);
    let effects = d.take_effects();
    // its monitor moved to the one that left, which is not minimised
    assert!(d.slot(0).expect("slot").monitor.is_some());
    assert!(d.slot(1).expect("slot").monitor.is_none());
    assert!(!minimised(&effects, 0));
    assert_eq!(memory(&d, 0).release, Release::Lowered);
    assert_eq!(memory(&d, 1).release, Release::Keep);
}

#[test]
fn the_background_share_is_saved_kept_to_its_choices_and_said_at_once() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    start(&mut d, 1, false, now);
    d.take_effects();
    d.set_background_memory_mb(2000);
    assert_eq!(d.background_memory_mb(), 2048);
    let effects = d.take_effects();
    assert!(effects.contains(&Effect::Save));
    assert!(effects.iter().any(|e| matches!(e, Effect::Send {
        slot: 1,
        message: ToViewer::Assign(a),
        ..
    } if a.memory.share_mb == 2048)));
    // the same again: nothing
    d.set_background_memory_mb(2048);
    assert!(d.take_effects().is_empty());
    d.set_background_memory_mb(10);
    assert_eq!(d.background_memory_mb(), memory::MIN_BACKGROUND_MB);
    // through the file, and a file of before the setting reads the default
    let config = d.config();
    assert_eq!(config.background_memory_mb, memory::MIN_BACKGROUND_MB);
    let json = serde_json::to_value(&config).expect("json");
    let read: Config = serde_json::from_value(json.clone()).expect("reads");
    assert_eq!(Desk::new(read).background_memory_mb(), memory::MIN_BACKGROUND_MB);
    let mut old = json;
    old.as_object_mut().expect("an object").remove("background_memory_mb");
    let read: Config = serde_json::from_value(old).expect("reads");
    assert_eq!(Desk::new(read).background_memory_mb(), memory::DEFAULT_BACKGROUND_MB);
    // a number out of the choices in the file is kept to them
    let wild = Config {
        background_memory_mb: 1_000_000,
        ..Config::default()
    };
    assert_eq!(Desk::new(wild).background_memory_mb(), memory::MAX_BACKGROUND_MB);
    // the settings window opens on the characters for these words
    for word in ["memory", "vram", "video-memory"] {
        assert_eq!(settings::Section::parse(word), Some(settings::Section::Characters));
    }
}
