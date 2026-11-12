//! Checkpoint 5's desk rules: which characters are watched, what each is
//! asked within the monitors' budget, and that a monitor nobody can see
//! asks nothing. (In a file of its own, as checkpoint 4's.)

use super::*;
use crate::monitor::{Order, Rate, Size, Spec};

fn desk(count: usize) -> (Desk, Instant) {
    (Desk::new(Config::demo(count)), Instant::now())
}

fn start(d: &mut Desk, slot: usize, activate: bool, now: Instant) -> u32 {
    d.launch(slot, activate);
    let launch = d.slot(slot).expect("slot").launch();
    d.connected(slot, launch, now);
    launch
}

fn assigned(d: &Desk, slot: usize) -> Assignment {
    d.slot(slot).expect("slot").sent.expect("sent").0
}

fn role(d: &Desk, slot: usize) -> Role {
    assigned(d, slot).role
}

fn spec(size: Size, rate: Rate) -> Spec {
    Spec { size, rate }
}

/// Every monitor that is open is on screen.
fn all_seen(d: &mut Desk, now: Instant) {
    d.monitors_seen([true; MAX_SLOTS], now);
}

fn minimises(effects: &[Effect]) -> Vec<usize> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                slot,
                message: ToViewer::Minimise,
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect()
}

#[test]
fn a_background_character_is_watched_only_while_its_monitor_is_open_and_seen() {
    let (mut d, now) = desk(3);
    for slot in 0..3 {
        start(&mut d, slot, slot == 0, now);
    }
    assert_eq!(d.watch(1), Watch::Closed);
    assert_eq!(role(&d, 1), Role::Background, "3D off by default (decision 1)");
    // opened, but not drawn in the window yet: nothing is asked
    d.set_monitor(1, Some(Spec::default()));
    assert!(d.take_effects().contains(&Effect::Save));
    assert_eq!(d.watch(1), Watch::Hidden);
    assert_eq!(role(&d, 1), Role::Background);
    // on screen: its viewer is asked, with what was chosen
    all_seen(&mut d, now);
    let order = Order {
        size: Size::Medium,
        interval_ms: 1000,
    };
    assert_eq!(role(&d, 1), Role::Monitor(order));
    assert_eq!(d.watch(1), Watch::Asked { order, lowered: false });
    // still a background character in everything else
    let a = assigned(&d, 1);
    assert_eq!((a.behind, a.sound_muted, a.voice_allowed), (Behind3d::Off, true, false));
    // the others are untouched: a monitor that is not open is never seen
    assert_eq!(role(&d, 2), Role::Background);
    assert_eq!(role(&d, 0), Role::Active);
    // the choice changes: a new order
    d.set_monitor(1, Some(spec(Size::Large, Rate::Two)));
    assert_eq!(
        role(&d, 1),
        Role::Monitor(Order {
            size: Size::Large,
            interval_ms: 500
        })
    );
    // closed: back to Off
    d.set_monitor(1, None);
    assert_eq!(role(&d, 1), Role::Background);
    assert_eq!(d.watch(1), Watch::Closed);
    // and opening it again waits to be seen again
    d.set_monitor(1, Some(Spec::default()));
    assert_eq!(role(&d, 1), Role::Background);
}

#[test]
fn a_hidden_or_minimised_host_pauses_every_monitor() {
    let (mut d, now) = desk(4);
    for slot in 0..4 {
        start(&mut d, slot, slot == 0, now);
        d.set_monitor(slot, Some(Spec::default()));
    }
    all_seen(&mut d, now);
    assert!((1..4).all(|slot| matches!(role(&d, slot), Role::Monitor(_))));
    d.take_effects();
    // the window is minimised or covered: nothing is on screen
    d.monitors_seen([false; MAX_SLOTS], now);
    assert!((1..4).all(|slot| role(&d, slot) == Role::Background));
    assert!((1..4).all(|slot| d.watch(slot) == Watch::Hidden));
    // every viewer was told, once
    let told: Vec<usize> = d
        .take_effects()
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                slot,
                message: ToViewer::Assign(a),
                ..
            } if a.role == Role::Background => Some(*slot),
            _ => None,
        })
        .collect();
    assert_eq!(told, vec![1, 2, 3]);
    // nothing changes: nothing is sent again
    d.monitors_seen([false; MAX_SLOTS], now);
    assert!(d.take_effects().is_empty());
    // back on screen: watched again; one scrolled away is not
    d.monitors_seen([true, true, false, true, true], now);
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
    assert_eq!(role(&d, 2), Role::Background);
    assert!(matches!(role(&d, 3), Role::Monitor(_)));
    // the monitors stay open in the settings all along
    assert!(d.config().slots.iter().all(|s| s.monitor == Some(Spec::default())));
}

#[test]
fn the_active_character_has_no_monitor_and_the_one_left_gets_its_own_back() {
    let (mut d, now) = desk(3);
    for slot in 0..3 {
        start(&mut d, slot, slot == 0, now);
        d.set_monitor(slot, Some(Spec::default()));
    }
    all_seen(&mut d, now);
    assert_eq!(d.watch(0), Watch::InFront);
    assert_eq!(role(&d, 0), Role::Active);
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
    // Monitor → Front, and Front → Monitor for the one left
    d.switch(1, now);
    assert_eq!(role(&d, 1), Role::Active);
    assert_eq!(d.watch(1), Watch::InFront);
    assert!(matches!(role(&d, 0), Role::Monitor(_)));
    assert!(matches!(role(&d, 2), Role::Monitor(_)));
    // the user clicks a watched window: the host follows, as before
    let launch = d.slot(2).expect("slot").launch();
    let status = Status {
        assign_seq: assigned(&d, 2).seq,
        screen: Screen::World,
        focused: true,
        ..Default::default()
    };
    d.status(2, launch, status, now);
    assert_eq!(d.active(), Some(2));
    assert_eq!(role(&d, 2), Role::Active);
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
}

#[test]
fn the_monitors_share_one_budget() {
    let (mut d, now) = desk(5);
    for slot in 0..5 {
        start(&mut d, slot, slot == 0, now);
    }
    // the acceptance: four monitors at one picture a second, as chosen
    for slot in 1..5 {
        d.set_monitor(slot, Some(spec(Size::Large, Rate::One)));
    }
    all_seen(&mut d, now);
    for slot in 1..5 {
        assert!(matches!(d.watch(slot), Watch::Asked { order, lowered: false } if order.interval_ms == 1000));
    }
    // all four at the fastest rate: every one lowered alike, within the budget
    for slot in 1..5 {
        d.set_monitor(slot, Some(spec(Size::Large, Rate::Five)));
    }
    let orders: Vec<Order> = (1..5)
        .map(|slot| match d.watch(slot) {
            Watch::Asked { order, lowered: true } => order,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(orders.iter().all(|o| *o == orders[0] && o.interval_ms > 200));
    assert!(monitor::load(&orders) <= monitor::BUDGET);
    assert!((1..5).all(|slot| role(&d, slot) == Role::Monitor(orders[0])));
    // one of them closes: the others get more of the budget, at once
    d.set_monitor(4, None);
    let fewer = match d.watch(1) {
        Watch::Asked { order, .. } => order,
        other => panic!("{other:?}"),
    };
    assert!(fewer.interval_ms < orders[0].interval_ms);
    assert_eq!(role(&d, 1), Role::Monitor(fewer));
}

#[test]
fn no_more_than_four_monitors_run_at_once() {
    let (mut d, now) = desk(5);
    // no active character yet: five could be on screen
    for slot in 0..5 {
        d.launch(slot, false);
    }
    for slot in 0..5 {
        d.set_monitor(slot, Some(spec(Size::Small, Rate::One)));
    }
    for slot in 1..5 {
        let launch = d.slot(slot).expect("slot").launch();
        d.connected(slot, launch, now);
    }
    // (the first to report became the active one: bring nobody forward)
    all_seen(&mut d, now);
    let watched = (0..5).filter(|slot| matches!(d.watch(*slot), Watch::Asked { .. })).count();
    assert!(watched <= MAX_MONITORS);
    assert_eq!(d.watch(d.active().expect("active")), Watch::InFront);
    assert_eq!(d.watch(0), Watch::NoViewer, "still starting");
}

#[test]
fn a_character_whose_monitor_is_open_is_not_minimised_behind() {
    let (mut d, now) = desk(3);
    for slot in 0..3 {
        start(&mut d, slot, slot == 0, now);
        d.set_standby(slot, Standby::Economy);
    }
    d.set_monitor(0, Some(Spec::default()));
    all_seen(&mut d, now);
    d.take_effects();
    // 0 is left for 1: Economy would minimise it, its monitor keeps it behind
    d.switch(1, now);
    assert_eq!(minimises(&d.take_effects()), Vec::<usize>::new());
    assert_eq!(assigned(&d, 0).standby, Standby::Warm);
    assert!(matches!(role(&d, 0), Role::Monitor(_)));
    // 1 has no monitor: minimised when left, as before
    d.switch(2, now);
    assert_eq!(minimises(&d.take_effects()), vec![1]);
    assert_eq!(assigned(&d, 1).standby, Standby::Economy);
    // the choice itself is kept for when the monitor closes
    assert_eq!(d.slot(0).expect("slot").standby, Standby::Economy);
    d.set_monitor(0, None);
    assert_eq!(assigned(&d, 0).standby, Standby::Economy);
}

#[test]
fn a_viewer_that_ends_or_closes_is_not_watched_and_a_restart_is_again() {
    let (mut d, now) = desk(2);
    start(&mut d, 0, true, now);
    let first = start(&mut d, 1, false, now);
    d.set_monitor(1, Some(Spec::default()));
    all_seen(&mut d, now);
    assert!(matches!(d.watch(1), Watch::Asked { .. }));
    d.close(1);
    assert_eq!(d.watch(1), Watch::NoViewer);
    d.exited(1, first, true, now);
    assert_eq!(d.watch(1), Watch::NoViewer);
    // started again: the monitor is still open and still on screen
    let second = start(&mut d, 1, false, now);
    assert_ne!(first, second);
    assert!(matches!(role(&d, 1), Role::Monitor(_)));
}

#[test]
fn only_pictures_that_were_asked_for_are_shown() {
    let (mut d, now) = desk(3);
    start(&mut d, 0, true, now);
    let launch = start(&mut d, 1, false, now);
    start(&mut d, 2, false, now);
    let picture = |w, h| FrameHeader::new(1, 1, w, h).expect("a header");
    // no monitor: nothing is shown, whatever a viewer sends
    assert!(!d.frame_wanted(1, launch, &picture(320, 180)));
    d.set_monitor(1, Some(spec(Size::Medium, Rate::One)));
    assert!(!d.frame_wanted(1, launch, &picture(320, 180)), "not on screen yet");
    all_seen(&mut d, now);
    assert!(d.frame_wanted(1, launch, &picture(480, 270)));
    assert!(d.frame_wanted(1, launch, &picture(360, 270)), "a 4:3 window");
    // larger than the size asked (taken before the size was lowered)
    assert!(!d.frame_wanted(1, launch, &picture(640, 360)));
    // another launch of the slot, another slot, a header that lies
    assert!(!d.frame_wanted(1, launch + 1, &picture(480, 270)));
    assert!(!d.frame_wanted(2, launch, &picture(480, 270)));
    assert!(!d.frame_wanted(9, launch, &picture(480, 270)));
    let lying = FrameHeader {
        bytes: 5,
        ..picture(480, 270)
    };
    assert!(!d.frame_wanted(1, launch, &lying));
    // hidden, in front, closed: what is still on its way is dropped
    d.monitors_seen([false; MAX_SLOTS], now);
    assert!(!d.frame_wanted(1, launch, &picture(480, 270)));
    all_seen(&mut d, now);
    d.switch(1, now);
    assert!(!d.frame_wanted(1, launch, &picture(480, 270)));
    d.switch(0, now);
    assert!(d.frame_wanted(1, launch, &picture(480, 270)));
    d.set_monitor(1, None);
    assert!(!d.frame_wanted(1, launch, &picture(480, 270)));
}

#[test]
fn the_monitors_are_kept_in_the_settings_file() {
    let (mut d, _) = desk(2);
    d.set_monitor(1, Some(spec(Size::Small, Rate::Half)));
    let config = d.config();
    assert_eq!(config.slots[0].monitor, None);
    assert_eq!(config.slots[1].monitor, Some(spec(Size::Small, Rate::Half)));
    let text = serde_json::to_string(&config).expect("written");
    let read: Config = serde_json::from_str(&text).expect("read");
    let d = Desk::new(read);
    assert_eq!(d.slot(1).expect("slot").monitor, Some(spec(Size::Small, Rate::Half)));
    assert_eq!(d.watch(1), Watch::NoViewer);
    // a file of an earlier version has none
    let old: Config = serde_json::from_str(r#"{"slots":[{"slot":0,"profile":"a"}]}"#).expect("read");
    assert_eq!(old.slots[0].monitor, None);
}
