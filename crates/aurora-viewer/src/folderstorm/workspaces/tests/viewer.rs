//! Frame tests of the viewer window in a layout: a pretend window that
//! runs what the layouts ask (`window::Pretend`), pretend monitors, the
//! same floaters as the other frame tests.

use super::*;
use crate::folderstorm::workspaces::apply::Notice;
use crate::folderstorm::workspaces::menu::{self, Request};
use crate::folderstorm::workspaces::store::{MonitorPolicy, WindowFrame};
use crate::folderstorm::workspaces::window::{Command, Pretend};

const LEFT: [i32; 4] = [0, 0, 1920, 1080];
const RIGHT: [i32; 4] = [1920, 0, 1920, 1080];

/// A viewer whose window lies over two monitors, with "Wide" saved there
/// (the window, the world on the right monitor, the windows in the left
/// margin; `lock`: with its lock box) and "Single" saved on the left
/// monitor alone, in use.
fn two_monitors(lock: bool) -> Viewer {
    let mut v = Viewer::new(3840.0, 1040.0);
    v.window = Some(Pretend::new(0, 0, 3840, 1040));
    v.monitors = vec![LEFT, RIGHT];
    v.frames(4);
    v.world = ViewportSettings {
        enabled: true,
        left: 1920,
        bars_around: true,
        ..Default::default()
    };
    v.put(INVENTORY, 10.0, 50.0, 900.0, 800.0);
    v.put(PEOPLE, 920.0, 50.0, 400.0, 600.0);
    v.put(MAP, 1330.0, 50.0, 560.0, 500.0);
    v.frames(4);
    v.save_new(
        "Wide",
        Parts {
            lock,
            ..Default::default()
        },
    )
    .expect("saved");
    v.settle();
    // one monitor: the window on it, no viewport
    v.monitors = vec![LEFT];
    v.window = Some(Pretend::new(0, 0, 1920, 1040));
    v.world = ViewportSettings::default();
    v.frames(4);
    v.put(INVENTORY, 1500.0, 60.0, 380.0, 500.0);
    v.put(PEOPLE, 20.0, 60.0, 300.0, 420.0);
    v.put(MAP, 400.0, 200.0, 600.0, 500.0);
    v.frames(4);
    v.save_new("Single", Parts::default()).expect("saved");
    // (the watch settles on the one monitor)
    v.frames(200);
    v.saved = false;
    v
}

impl Viewer {
    fn pretend(&mut self) -> &mut Pretend {
        self.window.as_mut().expect("a pretend window")
    }

    fn seen(&self) -> crate::folderstorm::workspaces::window::Seen {
        self.window.as_ref().expect("a pretend window").seen
    }

    /// The second monitor is plugged in again.
    fn plug(&mut self) {
        self.monitors = vec![LEFT, RIGHT];
        self.frames(200);
    }

    fn notice(&self) -> Option<Notice> {
        apply::notice(&self.ctx)
    }
}

#[test]
fn a_layout_saves_the_viewer_window_with_its_monitors() {
    let v = two_monitors(true);
    let wide = v.layout("Wide");
    assert_eq!(
        wide.window,
        Some(WindowFrame::Normal {
            x: Some(0),
            y: Some(0),
            width: 3840,
            height: 1040,
            maximized: false,
            locked: true,
        })
    );
    assert_eq!(wide.monitors.len(), 2);
    let single = v.layout("Single");
    assert!(matches!(
        single.window,
        Some(WindowFrame::Normal {
            width: 1920,
            locked: false,
            ..
        })
    ));
    assert_eq!(single.monitors.len(), 1);
    assert!(!v.store.window_held, "“Single” has no lock");
}

#[test]
fn applying_puts_the_viewer_window_back_then_the_windows_in_it() {
    let mut v = two_monitors(false);
    v.plug();
    v.pretend().lag = 20;
    let report = v.apply("Wide");
    assert_eq!((report.missing, report.left, report.unlocked, report.no_window), (0, false, false, false));
    // the window has not answered yet: the layout's windows wait for it
    v.frames(10);
    assert_eq!(v.seen().inner, (1920, 1040));
    assert!(v.rect(INVENTORY).left() > 1000.0, "not placed in the small window");
    v.settle();
    v.settle();
    assert_eq!((v.seen().outer, v.seen().inner), (Some((0, 0)), (3840, 1040)));
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(10.0, 50.0), vec2(900.0, 800.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
    assert!(
        close(v.rect(MAP), Rect::from_min_size(pos2(1330.0, 50.0), vec2(560.0, 500.0))),
        "{:?}",
        v.rect(MAP)
    );
    assert_eq!(apply::report(&v.ctx).map(|report| report.fitted), Some(0), "placed as saved, in the window as saved");
    assert!(!v.modified());
    // and back to one monitor: the window first, the windows where they fit
    v.apply("Single");
    v.settle();
    v.settle();
    assert_eq!(v.seen().inner, (1920, 1040));
    assert!(close(v.rect(INVENTORY), Rect::from_min_size(pos2(1500.0, 60.0), vec2(380.0, 500.0))));
    assert!(!v.modified());
}

#[test]
fn a_locked_layout_locks_the_window_until_it_is_unlocked() {
    let mut v = two_monitors(true);
    v.plug();
    assert!(v.seen().resizable);
    v.apply("Wide");
    v.settle();
    assert!(v.store.window_held);
    assert!(!v.seen().resizable && v.seen().decorated);
    // Unlock: at once, and the window keeps its place
    v.pretend().log.clear();
    assert!(apply::unlock(&mut v.store));
    v.frames(2);
    assert_eq!(v.pretend().log, [Command::Resizable(true)]);
    assert_eq!(v.seen().inner, (3840, 1040));
    assert!(!apply::unlock(&mut v.store), "nothing held any more");
    // a layout without a lock unlocks too
    v.apply("Wide");
    v.settle();
    assert!(!v.seen().resizable);
    v.apply("Single");
    v.settle();
    assert!(v.seen().resizable);
    assert!(!v.store.window_held);
}

#[test]
fn saving_with_the_lock_box_locks_at_once() {
    let mut v = two_monitors(false);
    v.save_new(
        "Locked",
        Parts {
            lock: true,
            ..Default::default()
        },
    )
    .expect("saved");
    v.frames(3);
    assert!(v.store.window_held);
    assert!(!v.seen().resizable);
    assert_eq!(v.pretend().log.last(), Some(&Command::Resizable(false)));
    assert_eq!(v.seen().inner, (1920, 1040), "the window is not moved by a save");
}

#[test]
fn a_restart_locks_again_when_the_monitors_are_all_there() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    let mut next = v.restarted();
    assert!(next.seen().resizable, "a new window is Aurora's own");
    next.settle();
    assert!(!next.seen().resizable);
    assert!(next.store.window_held);
    assert_eq!(next.pretend().log, [Command::Resizable(false)], "locked, and not moved");
    // the same restart with the right monitor unplugged: left free
    let mut next = v.restarted();
    next.monitors = vec![LEFT];
    next.settle();
    assert!(next.seen().resizable);
    assert!(!next.store.window_held);
    assert!(next.pretend().log.is_empty());
    assert!(next.saved, "the settings no longer say the window is held");
}

#[test]
fn a_window_made_anew_is_locked_again() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    // a GPU recovery: another window at the same place
    let mut fresh = Pretend::new(0, 0, 3840, 1040);
    fresh.seen.id = 2;
    v.window = Some(fresh);
    v.frames(3);
    assert!(!v.seen().resizable);
    assert_eq!(v.pretend().log, [Command::Resizable(false)]);
}

#[test]
fn entering_full_screen_unlocks() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    assert!(!v.seen().resizable);
    // Aurora's own full screen (its shortcut, its preference)
    v.pretend().send(&[Command::Fullscreen(true)]);
    v.pretend().log.clear();
    v.frames(3);
    assert!(!v.store.window_held);
    assert!(v.seen().resizable);
    assert_eq!(v.pretend().log, [Command::Resizable(true)]);
}

#[test]
fn a_lost_monitor_unlocks_and_offers_the_layout_saved_without_it() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    assert_eq!(v.notice(), None);
    // the right monitor is unplugged; Windows squeezes the window itself
    v.monitors = vec![LEFT];
    v.pretend().seen.inner = (1920, 1040);
    v.frames(60);
    assert!(v.store.window_held, "not before the setup has held two seconds");
    assert_eq!(v.notice(), None);
    v.frames(150);
    assert!(!v.store.window_held);
    assert!(v.seen().resizable);
    assert_eq!(
        v.notice(),
        Some(Notice::Offer {
            name: "Single".into(),
            unlocked: true,
        })
    );
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"), "nothing is applied without a yes");
    // Apply, from the card
    menu::ask(&v.ctx, Request::Apply("Single".into()));
    v.settle();
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert_eq!(v.notice(), None);
    assert!(!v.world.enabled);
}

#[test]
fn not_now_takes_the_card_away_and_changes_nothing() {
    let mut v = two_monitors(false);
    v.plug();
    assert_eq!(
        v.notice(),
        Some(Notice::Offer {
            name: "Wide".into(),
            unlocked: false,
        })
    );
    menu::ask(&v.ctx, Request::Dismiss);
    v.frames(3);
    assert_eq!(v.notice(), None);
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert_eq!(v.seen().inner, (1920, 1040));
}

#[test]
fn the_layout_in_use_already_fits_the_new_setup_nothing_is_offered() {
    let mut v = two_monitors(false);
    v.plug();
    menu::ask(&v.ctx, Request::Apply("Wide".into()));
    v.settle();
    // unplugged and plugged back within the two seconds: no change at all
    v.monitors = vec![LEFT];
    v.frames(60);
    v.monitors = vec![LEFT, RIGHT];
    v.frames(200);
    assert_eq!(v.notice(), None);
    // a third monitor no layout was saved on: nothing to offer
    v.monitors = vec![LEFT, RIGHT, [3840, 0, 1920, 1080]];
    v.frames(200);
    assert_eq!(v.notice(), None);
    // back to two, the setup of the layout in use: nothing either
    v.monitors = vec![LEFT, RIGHT];
    v.frames(200);
    assert_eq!(v.notice(), None);
}

#[test]
fn the_matching_layout_is_applied_without_asking_when_chosen_so() {
    let mut v = two_monitors(false);
    v.store.on_monitor_change = MonitorPolicy::Apply;
    v.plug();
    v.settle();
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert_eq!(v.seen().inner, (3840, 1040));
    assert!(v.world.enabled);
    assert_eq!(v.notice(), Some(Notice::Applied { name: "Wide".into() }));
    // Go back, from the card
    menu::ask(&v.ctx, Request::Previous);
    v.settle();
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert_eq!(v.seen().inner, (1920, 1040));
    assert_eq!(v.notice(), None);
}

#[test]
fn nothing_is_shown_when_the_user_chose_so() {
    let mut v = two_monitors(true);
    v.store.on_monitor_change = MonitorPolicy::Nothing;
    v.plug();
    assert_eq!(v.notice(), None);
    v.apply("Wide");
    v.settle();
    v.monitors = vec![LEFT];
    v.frames(200);
    assert_eq!(v.notice(), None);
    assert!(!v.store.window_held, "the lock is still lifted");
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
}

#[test]
fn a_layout_applied_with_a_monitor_missing_is_cut_to_the_monitors_and_not_locked() {
    let mut v = two_monitors(true);
    let report = v.apply("Wide");
    assert_eq!((report.missing, report.left, report.unlocked), (1, false, true));
    v.settle();
    v.settle();
    assert_eq!(v.seen().inner, (1920, 1040), "never wider than the monitors there are");
    assert!(v.seen().resizable);
    assert!(!v.store.window_held);
    // the saved layout is not rewritten
    assert!(matches!(v.layout("Wide").window, Some(WindowFrame::Normal { width: 3840, .. })));
    // its windows are fitted to the window there is
    assert!(apply::report(&v.ctx).is_some_and(|report| report.fitted > 0));
    assert!(v.rect(MAP).right() <= 1920.5);
}

#[test]
fn previous_brings_the_viewer_window_and_its_lock_back() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    v.settle();
    assert!(!v.seen().resizable);
    v.apply("Single");
    v.settle();
    v.settle();
    assert_eq!(v.seen().inner, (1920, 1040));
    assert!(v.seen().resizable);
    assert!(v.previous());
    v.settle();
    v.settle();
    assert_eq!(v.seen().inner, (3840, 1040));
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert!(v.store.window_held);
    assert!(!v.seen().resizable);
    // and again
    assert!(v.previous());
    v.settle();
    v.settle();
    assert_eq!(v.seen().inner, (1920, 1040));
    assert!(v.seen().resizable);
}

#[test]
fn a_layout_without_the_part_leaves_the_viewer_window_alone() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    v.save_new(
        "Windows only",
        Parts {
            window: false,
            world: false,
            ..Default::default()
        },
    )
    .expect("saved");
    v.settle();
    assert_eq!(v.layout("Windows only").window, None);
    // (saving a layout without a lock lets the window go)
    assert!(!v.store.window_held);
    v.apply("Wide");
    v.settle();
    v.pretend().log.clear();
    v.apply("Windows only");
    v.settle();
    assert_eq!(v.pretend().log, [Command::Resizable(true)], "unlocked, neither moved nor resized");
    assert_eq!(v.seen().inner, (3840, 1040));
    // Previous after it does not move the window either
    v.pretend().log.clear();
    assert!(v.previous());
    v.settle();
    assert!(v.pretend().log.is_empty());
}

#[test]
fn moving_or_resizing_the_viewer_window_marks_the_layout_modified() {
    let mut v = two_monitors(false);
    v.plug();
    menu::ask(&v.ctx, Request::Dismiss);
    v.apply("Wide");
    v.settle();
    v.settle();
    assert!(!v.modified());
    // a few pixels are the system's own rounding
    v.pretend().seen.outer = Some((3, 2));
    v.settle();
    assert!(!v.modified());
    v.pretend().seen.outer = Some((40, 2));
    v.settle();
    assert!(v.modified());
    // Update takes the window as it is
    let ctx = v.ctx.clone();
    let (index, _) = v.store.find("Wide").expect("in the list");
    apply::update(&ctx, &mut v.live(), index).expect("updated");
    v.settle();
    assert!(!v.modified());
    assert!(matches!(
        v.layout("Wide").window,
        Some(WindowFrame::Normal {
            x: Some(40),
            y: Some(2),
            ..
        })
    ));
    // a layout without the part does not watch the window
    v.save_new(
        "World only",
        Parts {
            window: false,
            windows: false,
            ..Default::default()
        },
    )
    .expect("saved");
    v.settle();
    v.pretend().seen.outer = Some((300, 200));
    v.settle();
    assert!(!v.modified());
}

#[test]
fn update_keeps_the_layout_s_lock_box() {
    let mut v = two_monitors(true);
    v.plug();
    v.apply("Wide");
    v.settle();
    v.settle();
    apply::unlock(&mut v.store);
    v.frames(3);
    v.pretend().seen.inner = (3000, 1000);
    v.settle();
    let ctx = v.ctx.clone();
    let (index, _) = v.store.find("Wide").expect("in the list");
    apply::update(&ctx, &mut v.live(), index).expect("updated");
    v.frames(3);
    assert_eq!(
        v.layout("Wide").window,
        Some(WindowFrame::Normal {
            x: Some(0),
            y: Some(0),
            width: 3000,
            height: 1000,
            maximized: false,
            locked: true,
        })
    );
    assert!(!v.store.window_held, "unlocked by hand stays unlocked");
    assert!(v.seen().resizable);
}

#[test]
fn borderless_covers_its_monitors_and_gets_its_border_back() {
    let mut v = two_monitors(false);
    v.plug();
    let (index, _) = v.store.find("Wide").expect("in the list");
    apply::set_window(
        &mut v.store,
        index,
        WindowFrame::Borderless {
            monitors: vec![0, 1],
            locked: true,
        },
    )
    .expect("edited");
    v.apply("Wide");
    v.settle();
    v.settle();
    assert_eq!((v.seen().outer, v.seen().inner), (Some((0, 0)), (3840, 1080)));
    assert!(!v.seen().decorated && !v.seen().resizable);
    assert!(v.store.window_held);
    // saved again as it is: read back as borderless
    v.save_new("Borderless", Parts::default()).expect("saved");
    assert_eq!(
        v.layout("Borderless").window,
        Some(WindowFrame::Borderless {
            monitors: vec![0, 1],
            locked: true,
        })
    );
    // a restart puts it back over its monitors without its border
    let mut next = v.restarted();
    assert!(next.seen().decorated);
    next.settle();
    assert!(!next.seen().decorated && !next.seen().resizable);
    assert_eq!((next.seen().outer, next.seen().inner), (Some((0, 0)), (3840, 1080)));
    // Unlock: border and resizing back, the place kept
    apply::unlock(&mut v.store);
    v.frames(3);
    assert!(v.seen().decorated && v.seen().resizable);
    assert_eq!(v.seen().inner, (3840, 1080));
    // a monitor gone: over the one left, never locked
    v.monitors = vec![LEFT];
    v.frames(200);
    let report = v.apply("Wide");
    assert_eq!((report.missing, report.unlocked), (1, true));
    v.settle();
    v.settle();
    assert_eq!((v.seen().outer, v.seen().inner), (Some((0, 0)), (1920, 1080)));
    assert!(!v.seen().decorated);
    assert!(v.seen().resizable);
}

#[test]
fn full_screen_is_entered_on_its_monitor() {
    let mut v = two_monitors(false);
    v.plug();
    let (index, _) = v.store.find("Wide").expect("in the list");
    apply::set_window(&mut v.store, index, WindowFrame::Fullscreen { monitor: 1 }).expect("edited");
    v.pretend().log.clear();
    v.apply("Wide");
    v.settle();
    assert_eq!(v.pretend().log, [Command::Position(2400, 270), Command::Fullscreen(true)]);
    assert!(v.seen().fullscreen);
    assert!(!v.store.window_held);
    // a normal window again: full screen is left first
    v.apply("Single");
    v.settle();
    v.settle();
    assert!(!v.seen().fullscreen);
    assert_eq!((v.seen().outer, v.seen().inner), (Some((0, 0)), (1920, 1040)));
}

#[test]
fn without_a_window_to_drive_the_rest_of_the_layout_still_applies() {
    let mut v = two_monitors(true);
    v.plug();
    // a capture, a forced size: the app describes no window
    v.window = None;
    v.frames(3);
    let report = v.apply("Wide");
    assert!(report.no_window);
    v.settle();
    assert!(v.world.enabled);
    assert!(!v.store.window_held);
    assert!(close(v.rect(PEOPLE), Rect::from_min_size(pos2(920.0, 50.0), vec2(400.0, 600.0))));
    // and a save leaves the part out
    v.save_new("No window", Parts::default()).expect("saved");
    assert_eq!(v.layout("No window").window, None);
}

#[test]
fn editing_the_lock_box_of_the_layout_in_use_takes_effect_at_once() {
    let mut v = two_monitors(false);
    v.plug();
    menu::ask(&v.ctx, Request::Apply("Wide".into()));
    v.settle();
    v.settle();
    let (index, layout) = v.store.find("Wide").expect("in the list");
    let frame = layout.window.clone().expect("the part");
    apply::set_window(&mut v.store, index, frame.clone().with_lock(true)).expect("edited");
    v.frames(3);
    assert!(!v.seen().resizable);
    assert!(!v.modified(), "the window is as the layout has it");
    apply::set_window(&mut v.store, index, frame).expect("edited");
    v.frames(3);
    assert!(v.seen().resizable);
    // another mode waits for Apply, and says so
    apply::set_window(&mut v.store, index, WindowFrame::Fullscreen { monitor: 0 }).expect("edited");
    v.frames(3);
    assert!(v.modified());
    assert!(!v.seen().fullscreen);
    // a monitor the layout does not have is refused
    assert_eq!(
        apply::set_window(&mut v.store, index, WindowFrame::Fullscreen { monitor: 5 }),
        Err(StoreError::Invalid)
    );
}

#[test]
fn the_menu_s_requests_are_run_before_the_next_frame() {
    let mut v = two_monitors(true);
    v.plug();
    menu::ask(&v.ctx, Request::Apply("wide".into()));
    v.settle();
    assert_eq!(v.store.active_layout.as_deref(), Some("Wide"));
    assert!(v.store.window_held);
    menu::ask(&v.ctx, Request::Unlock);
    v.frames(3);
    assert!(!v.store.window_held && v.seen().resizable);
    menu::ask(&v.ctx, Request::Previous);
    v.settle();
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
    assert!(!v.preferences);
    menu::ask(&v.ctx, Request::Preferences);
    v.frames(2);
    assert!(v.preferences);
    // a layout that is no longer there: nothing happens
    menu::ask(&v.ctx, Request::Apply("Gone".into()));
    v.frames(2);
    assert_eq!(v.store.active_layout.as_deref(), Some("Single"));
}

#[test]
fn a_window_first_shown_after_the_baseline_is_watched_from_then_on() {
    let mut v = two_layouts();
    // a restart with the world map closed: the viewer does not know where
    // it is until it is shown
    v.open.remove(MAP);
    let mut next = v.restarted();
    next.settle();
    assert!(!next.modified());
    next.open.insert(MAP);
    next.settle();
    assert!(!next.modified(), "where Aurora opens it is not a move");
    next.put(MAP, 300.0, 250.0, 600.0, 500.0);
    next.settle();
    assert!(next.modified(), "moved since it was first shown");
}
