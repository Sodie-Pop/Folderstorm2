//! Monitor setups, and what a saved viewer window becomes on the monitors
//! of now (folderstorm/plans/workspaces.md section 8.5; checkpoint 4 in
//! detail, point 2). Pure: desktop pixels in, a [`Plan`] out.
//!
//! A layout remembers the monitors it was saved on (place, size, scale;
//! never a name). They tell whether the setup of now is the same one, which
//! monitors of a layout are missing, and where a full-screen or borderless
//! window goes.

use super::store::{Monitor, Store, WindowFrame};
use super::window::{Goal, Hold, Plan, Seen, to_i32};

/// Two monitors this close in place and size, in pixels, are the same one.
const NEAR: i64 = 2;
/// Two scales this close are the same.
const SCALE: f32 = 0.01;
/// The viewer's smallest window, in points (`app/gpu_recovery.rs`,
/// `open_window`).
const MIN_POINTS: (f32, f32) = (800.0, 560.0);
/// Seconds a new setup must hold before it counts: Windows moves the
/// viewer window itself while monitors come and go.
pub const SETTLED_AFTER: f64 = 2.0;

/// The same monitor by place and size (what a borderless or full-screen
/// window needs of it).
pub fn same_place(a: &Monitor, b: &Monitor) -> bool {
    let near = |a: i64, b: i64| (a - b).abs() <= NEAR;
    near(a.x.into(), b.x.into()) && near(a.y.into(), b.y.into()) && near(a.width.into(), b.width.into()) && near(a.height.into(), b.height.into())
}

fn same(a: &Monitor, b: &Monitor) -> bool {
    same_place(a, b) && (a.scale - b.scale).abs() <= SCALE
}

/// Whether two setups hold the same monitors, in any order.
pub fn same_setup(a: &[Monitor], b: &[Monitor]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // each monitor of `a` takes a partner of `b` no other has taken
    let mut taken = vec![false; b.len()];
    a.iter().all(|monitor| {
        let partner = (0..b.len()).find(|n| !taken[*n] && same(monitor, &b[*n]));
        partner.is_some_and(|n| {
            taken[n] = true;
            true
        })
    })
}

/// The monitor of `now` standing where `monitor` was.
pub fn present<'a>(monitor: &Monitor, now: &'a [Monitor]) -> Option<&'a Monitor> {
    now.iter().find(|other| same_place(monitor, other))
}

/// How many monitors of `saved` are not in `now`.
pub fn missing(saved: &[Monitor], now: &[Monitor]) -> usize {
    saved.iter().filter(|monitor| present(monitor, now).is_none()).count()
}

/// The rectangle covering `monitors` (x, y, width, height); None for none.
pub fn cover<'a>(monitors: impl IntoIterator<Item = &'a Monitor>) -> Option<[i32; 4]> {
    let mut all: Option<[i64; 4]> = None;
    for m in monitors {
        let (left, top) = (i64::from(m.x), i64::from(m.y));
        let (right, bottom) = (left + i64::from(m.width), top + i64::from(m.height));
        all = Some(match all {
            None => [left, top, right, bottom],
            Some([l, t, r, b]) => [l.min(left), t.min(top), r.max(right), b.max(bottom)],
        });
    }
    let [left, top, right, bottom] = all?;
    let clamp = |v: i64| i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX });
    Some([clamp(left), clamp(top), clamp(right - left), clamp(bottom - top)])
}

/// Whether a window whose outer corner is at (`x`, `y`) has its title bar
/// on a monitor: the test `App::resumed` makes before it reopens the
/// window where it was (a point 40 pixels right and 10 down of the corner).
pub fn title_bar_on(x: i32, y: i32, now: &[Monitor]) -> bool {
    let (px, py) = (i64::from(x) + 40, i64::from(y) + 10);
    now.iter().any(|m| {
        let (left, top) = (i64::from(m.x), i64::from(m.y));
        px >= left && px < left + i64::from(m.width) && py >= top && py < top + i64::from(m.height)
    })
}

/// The place in `monitors` of the one holding the point (`x`, `y`).
pub fn monitor_at(x: i64, y: i64, monitors: &[Monitor]) -> Option<usize> {
    monitors.iter().position(|m| {
        let (left, top) = (i64::from(m.x), i64::from(m.y));
        x >= left && x < left + i64::from(m.width) && y >= top && y < top + i64::from(m.height)
    })
}

/// What a saved viewer window asks of the window of now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    /// None: the window is left alone.
    pub plan: Option<Plan>,
    /// Monitors of the layout that are not there.
    pub missing: usize,
    /// The window could not go where the layout has it: it stays where it
    /// is (its size may still change).
    pub left: bool,
    /// The layout asks for a lock that is not given: a monitor is missing.
    pub unlocked: bool,
}

/// The rules of section 8.5: `frame`, saved on the monitors `saved`, for
/// the monitors of `now` (none where the system does not list them) and
/// the window as it is.
pub fn resolve(frame: &WindowFrame, saved: &[Monitor], now: &[Monitor], seen: &Seen) -> Asked {
    let known = !now.is_empty();
    let missing = if known { missing(saved, now) } else { 0 };
    let lock = |locked: bool| locked && missing == 0;
    match frame {
        WindowFrame::Normal {
            x,
            y,
            width,
            height,
            maximized,
            locked,
        } => {
            let wanted = x.zip(*y);
            let at = wanted.filter(|(x, y)| known && seen.outer.is_some() && title_bar_on(*x, *y, now));
            // never larger than the monitors there are, never smaller than
            // the viewer's own minimum
            let most = cover(now).map_or((u32::MAX, u32::MAX), |[_, _, w, h]| (w.max(1) as u32, h.max(1) as u32));
            let scale = if seen.scale.is_finite() && seen.scale > 0.0 { seen.scale } else { 1.0 };
            let least = ((MIN_POINTS.0 * scale).ceil() as u32, (MIN_POINTS.1 * scale).ceil() as u32);
            let size = ((*width).min(most.0).max(least.0), (*height).min(most.1).max(least.1));
            Asked {
                plan: Some(Plan {
                    goal: Goal::Normal {
                        at,
                        size,
                        maximized: *maximized,
                    },
                    hold: Hold {
                        decorated: true,
                        resizable: !lock(*locked),
                    },
                }),
                missing,
                left: known && wanted.is_some() && at.is_none(),
                unlocked: *locked && !lock(*locked),
            }
        }
        WindowFrame::Fullscreen { monitor } => {
            let wanted = saved.get(usize::from(*monitor));
            let there = wanted.and_then(|m| present(m, now));
            // already in full screen on that monitor: nothing to move
            let on_it = |m: &Monitor| {
                seen.fullscreen
                    && seen
                        .rect()
                        .is_some_and(|[x, y, w, h]| monitor_at(i64::from(x) + i64::from(w) / 2, i64::from(y) + i64::from(h) / 2, std::slice::from_ref(m)).is_some())
            };
            let on = there
                .filter(|m| seen.outer.is_some() && !on_it(m))
                .map(|m| (m.x.saturating_add(to_i32(m.width / 4)), m.y.saturating_add(to_i32(m.height / 4))));
            Asked {
                plan: Some(Plan {
                    goal: Goal::Fullscreen { on },
                    hold: Hold::FREE,
                }),
                missing,
                left: known && wanted.is_some() && there.is_none(),
                unlocked: false,
            }
        }
        WindowFrame::Borderless { monitors, locked } => {
            let chosen = monitors
                .iter()
                .filter_map(|n| saved.get(usize::from(*n)))
                .filter_map(|m| present(m, now));
            let rect = cover(chosen).filter(|_| seen.outer.is_some());
            Asked {
                plan: rect.map(|rect| Plan {
                    goal: Goal::Borderless { rect },
                    hold: Hold {
                        decorated: false,
                        resizable: !lock(*locked),
                    },
                }),
                missing,
                left: rect.is_none(),
                unlocked: *locked && (rect.is_none() || !lock(*locked)),
            }
        }
    }
}

/// The first usable layout of the list saved on exactly the setup `now`.
pub fn matching(store: &Store, now: &[Monitor]) -> Option<usize> {
    if now.is_empty() {
        return None;
    }
    (0..store.layouts.len()).find(|index| {
        store
            .layout(*index)
            .is_some_and(|layout| !layout.monitors.is_empty() && same_setup(&layout.monitors, now))
    })
}

/// Tells when the monitor setup has changed, once the new one has held
/// for [`SETTLED_AFTER`] seconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    settled: Option<Vec<Monitor>>,
    candidate: Option<(Vec<Monitor>, f64)>,
}

impl Watch {
    /// The setup last settled on.
    pub fn settled(&self) -> Option<&[Monitor]> {
        self.settled.as_deref()
    }

    /// The monitors seen at the time `t` (seconds). The new setup, once,
    /// when it has replaced the one before. A look without monitors (the
    /// system lists none, or not yet) tells nothing; the first setup seen
    /// is only noted.
    pub fn look(&mut self, now: &[Monitor], t: f64) -> Option<Vec<Monitor>> {
        if now.is_empty() {
            return None;
        }
        let Some(settled) = &self.settled else {
            self.settled = Some(now.to_vec());
            return None;
        };
        if same_setup(settled, now) {
            self.candidate = None;
            return None;
        }
        match &self.candidate {
            Some((candidate, since)) if same_setup(candidate, now) => {
                if t - since < SETTLED_AFTER {
                    return None;
                }
                self.candidate = None;
                self.settled = Some(now.to_vec());
                Some(now.to_vec())
            }
            _ => {
                self.candidate = Some((now.to_vec(), t));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::store::Layout;
    use crate::folderstorm::workspaces::window::Pretend;

    fn monitor(x: i32, y: i32, width: u32, height: u32) -> Monitor {
        Monitor {
            x,
            y,
            width,
            height,
            scale: 1.0,
        }
    }

    fn two() -> Vec<Monitor> {
        vec![monitor(0, 0, 1920, 1080), monitor(1920, 0, 1920, 1080)]
    }

    fn one() -> Vec<Monitor> {
        vec![monitor(0, 0, 1920, 1080)]
    }

    fn seen(x: i32, y: i32, width: u32, height: u32) -> Seen {
        Pretend::new(x, y, width, height).seen
    }

    fn normal(x: i32, y: i32, width: u32, height: u32, locked: bool) -> WindowFrame {
        WindowFrame::Normal {
            x: Some(x),
            y: Some(y),
            width,
            height,
            maximized: false,
            locked,
        }
    }

    #[test]
    fn a_setup_is_the_same_in_any_order() {
        let swapped = vec![monitor(1920, 0, 1920, 1080), monitor(0, 0, 1920, 1080)];
        assert!(same_setup(&two(), &swapped));
        assert!(!same_setup(&two(), &one()));
        assert!(!same_setup(&two(), &[monitor(0, 0, 1920, 1080), monitor(1920, 0, 2560, 1440)]));
        // another scale is another setup; a rounding of the scale is not
        let mut scaled = two();
        scaled[1].scale = 1.25;
        assert!(!same_setup(&two(), &scaled));
        scaled[1].scale = 1.004;
        assert!(same_setup(&two(), &scaled));
        // two monitors of one kind do not both pair with one
        let twins = vec![monitor(0, 0, 1920, 1080), monitor(0, 0, 1920, 1080)];
        assert!(!same_setup(&twins, &two()));
        assert!(same_setup(&[], &[]));
    }

    #[test]
    fn missing_monitors_are_counted_by_place_and_size() {
        assert_eq!(missing(&two(), &one()), 1);
        assert_eq!(missing(&two(), &two()), 0);
        assert_eq!(missing(&one(), &two()), 0, "a monitor more is not one missing");
        // the second one replaced by a larger one at the same place
        assert_eq!(missing(&two(), &[monitor(0, 0, 1920, 1080), monitor(1920, 0, 2560, 1440)]), 1);
    }

    #[test]
    fn the_cover_of_monitors_is_their_union() {
        assert_eq!(cover(&two()), Some([0, 0, 3840, 1080]));
        // one left of the main one and lower, another above
        let around = [monitor(-1280, 200, 1280, 1024), monitor(0, 0, 1920, 1080), monitor(0, -1080, 1920, 1080)];
        assert_eq!(cover(&around), Some([-1280, -1080, 3200, 2304]));
        assert_eq!(cover(&[]), None);
    }

    #[test]
    fn the_title_bar_test_is_the_one_of_the_start() {
        let left = [monitor(-1920, 0, 1920, 1080), monitor(0, 0, 1920, 1080)];
        assert!(title_bar_on(-1900, 20, &left));
        assert!(title_bar_on(-8, 0, &left), "the invisible border of Windows");
        assert!(!title_bar_on(-1960, 0, &left), "the corner 40 pixels left of every monitor");
        assert!(!title_bar_on(0, 1075, &left), "the title bar under the monitors");
        assert!(!title_bar_on(1920, 0, &left));
        assert!(!title_bar_on(0, 0, &[]));
    }

    #[test]
    fn a_normal_window_goes_back_where_it_was() {
        let asked = resolve(&normal(-8, 0, 3840, 1040, true), &two(), &two(), &seen(100, 100, 1280, 720));
        assert_eq!(
            asked,
            Asked {
                plan: Some(Plan {
                    goal: Goal::Normal {
                        at: Some((-8, 0)),
                        size: (3840, 1040),
                        maximized: false,
                    },
                    hold: Hold {
                        decorated: true,
                        resizable: false,
                    },
                }),
                missing: 0,
                left: false,
                unlocked: false,
            }
        );
    }

    #[test]
    fn a_normal_window_is_cut_to_the_monitors_and_not_locked_when_one_is_missing() {
        // saved over two monitors, the right one gone
        let asked = resolve(&normal(-8, 0, 3840, 1040, true), &two(), &one(), &seen(100, 100, 1280, 720));
        let plan = asked.plan.expect("still placed");
        assert_eq!(
            plan.goal,
            Goal::Normal {
                at: Some((-8, 0)),
                size: (1920, 1040),
                maximized: false,
            }
        );
        assert_eq!(plan.hold, Hold::FREE, "never locked while a monitor is missing");
        assert_eq!((asked.missing, asked.left, asked.unlocked), (1, false, true));
    }

    #[test]
    fn a_normal_window_whose_title_bar_would_be_off_every_monitor_stays_where_it_is() {
        // saved on the right monitor, which is gone
        let asked = resolve(&normal(2000, 100, 1600, 900, false), &two(), &one(), &seen(100, 100, 1280, 720));
        assert_eq!(
            asked.plan.map(|plan| plan.goal),
            Some(Goal::Normal {
                at: None,
                size: (1600, 900),
                maximized: false,
            })
        );
        assert!(asked.left);
        assert!(!asked.unlocked, "no lock was asked");
    }

    #[test]
    fn a_normal_window_is_never_smaller_than_the_viewer_allows() {
        let mut window = seen(0, 0, 1280, 720);
        window.scale = 1.5;
        let asked = resolve(&normal(0, 0, 300, 200, false), &one(), &one(), &window);
        assert_eq!(
            asked.plan.map(|plan| plan.goal),
            Some(Goal::Normal {
                at: Some((0, 0)),
                size: (1200, 840),
                maximized: false,
            })
        );
    }

    #[test]
    fn without_a_monitor_list_only_the_size_is_asked() {
        // Wayland: the frame was saved without a place, and there is none to give
        let mut window = seen(0, 0, 1280, 720);
        window.outer = None;
        let frame = WindowFrame::Normal {
            x: None,
            y: None,
            width: 1600,
            height: 900,
            maximized: false,
            locked: true,
        };
        let asked = resolve(&frame, &[], &[], &window);
        let plan = asked.plan.expect("sized");
        assert_eq!(
            plan.goal,
            Goal::Normal {
                at: None,
                size: (1600, 900),
                maximized: false,
            }
        );
        assert!(!plan.hold.resizable, "a lock needs no monitor");
        assert!(!asked.left);
        // a borderless window needs to know where the monitors are
        let asked = resolve(
            &WindowFrame::Borderless {
                monitors: vec![0],
                locked: true,
            },
            &one(),
            &[],
            &window,
        );
        assert_eq!((asked.plan, asked.left, asked.unlocked), (None, true, true));
        // full screen: on the monitor the window is on
        let asked = resolve(&WindowFrame::Fullscreen { monitor: 0 }, &[], &[], &window);
        assert_eq!(asked.plan.map(|plan| plan.goal), Some(Goal::Fullscreen { on: None }));
    }

    #[test]
    fn full_screen_goes_to_its_monitor_or_stays_on_the_one_it_is_on() {
        let frame = WindowFrame::Fullscreen { monitor: 1 };
        let asked = resolve(&frame, &two(), &two(), &seen(100, 100, 1280, 720));
        assert_eq!(asked.plan.map(|plan| plan.goal), Some(Goal::Fullscreen { on: Some((2400, 270)) }));
        assert!(!asked.left);
        // the monitor is gone
        let asked = resolve(&frame, &two(), &one(), &seen(100, 100, 1280, 720));
        assert_eq!(asked.plan.map(|plan| plan.goal), Some(Goal::Fullscreen { on: None }));
        assert_eq!((asked.missing, asked.left), (1, true));
        // already in full screen there
        let mut window = seen(1920, 0, 1920, 1080);
        window.fullscreen = true;
        let asked = resolve(&frame, &two(), &two(), &window);
        assert_eq!(asked.plan.map(|plan| plan.goal), Some(Goal::Fullscreen { on: None }));
        // in full screen on the other one
        let mut window = seen(0, 0, 1920, 1080);
        window.fullscreen = true;
        let asked = resolve(&frame, &two(), &two(), &window);
        assert_eq!(asked.plan.map(|plan| plan.goal), Some(Goal::Fullscreen { on: Some((2400, 270)) }));
    }

    #[test]
    fn borderless_covers_the_chosen_monitors_still_there() {
        let frame = WindowFrame::Borderless {
            monitors: vec![0, 1],
            locked: true,
        };
        let window = seen(100, 100, 1280, 720);
        let asked = resolve(&frame, &two(), &two(), &window);
        assert_eq!(
            asked.plan,
            Some(Plan {
                goal: Goal::Borderless { rect: [0, 0, 3840, 1080] },
                hold: Hold {
                    decorated: false,
                    resizable: false,
                },
            })
        );
        assert!(!asked.left && !asked.unlocked);
        // one of them gone: over the other, and not locked
        let asked = resolve(&frame, &two(), &one(), &window);
        assert_eq!(
            asked.plan,
            Some(Plan {
                goal: Goal::Borderless { rect: [0, 0, 1920, 1080] },
                hold: Hold {
                    decorated: false,
                    resizable: true,
                },
            })
        );
        assert_eq!((asked.missing, asked.left, asked.unlocked), (1, false, true));
        // none of them there: left alone
        let asked = resolve(&frame, &two(), &[monitor(0, 0, 2560, 1440)], &window);
        assert_eq!((asked.plan, asked.left, asked.unlocked), (None, true, true));
        // over the second one only, with a third monitor plugged in since
        let second = WindowFrame::Borderless {
            monitors: vec![1],
            locked: false,
        };
        let three = [monitor(0, 0, 1920, 1080), monitor(1920, 0, 1920, 1080), monitor(3840, 0, 1920, 1080)];
        let asked = resolve(&second, &two(), &three, &window);
        assert_eq!(
            asked.plan.map(|plan| plan.goal),
            Some(Goal::Borderless { rect: [1920, 0, 1920, 1080] })
        );
        assert_eq!(asked.missing, 0);
    }

    #[test]
    fn the_matching_layout_is_the_first_saved_on_this_setup() {
        let mut store = Store::default();
        for (name, monitors) in [("No monitors", Vec::new()), ("Wide", two()), ("Single", one()), ("Single too", one())] {
            store.add(Layout::new(name, None, None, monitors)).expect("added");
        }
        assert_eq!(matching(&store, &one()), Some(2));
        assert_eq!(matching(&store, &[monitor(1920, 0, 1920, 1080), monitor(0, 0, 1920, 1080)]), Some(1));
        assert_eq!(matching(&store, &[monitor(0, 0, 2560, 1440)]), None);
        assert_eq!(matching(&store, &[]), None, "a layout saved without monitors matches nothing");
    }

    #[test]
    fn a_new_setup_counts_once_it_has_held_two_seconds() {
        let mut watch = Watch::default();
        assert_eq!(watch.look(&two(), 0.0), None, "the first setup is only noted");
        assert_eq!(watch.look(&two(), 1.0), None);
        // the right monitor is unplugged
        assert_eq!(watch.look(&one(), 5.0), None);
        assert_eq!(watch.look(&one(), 6.0), None);
        assert_eq!(watch.look(&one(), 7.0), Some(one()));
        assert_eq!(watch.look(&one(), 8.0), None, "told once");
        assert_eq!(watch.settled(), Some(one().as_slice()));
    }

    #[test]
    fn a_setup_that_keeps_changing_or_comes_back_is_not_a_change() {
        let mut watch = Watch::default();
        watch.look(&two(), 0.0);
        // a flicker: gone for a second, then back
        assert_eq!(watch.look(&one(), 1.0), None);
        assert_eq!(watch.look(&two(), 2.0), None);
        assert_eq!(watch.look(&two(), 10.0), None);
        // changing twice: the wait starts again
        assert_eq!(watch.look(&one(), 11.0), None);
        let other = [monitor(0, 0, 2560, 1440)];
        assert_eq!(watch.look(&other, 12.5), None);
        assert_eq!(watch.look(&other, 14.0), None);
        assert_eq!(watch.look(&other, 14.6), Some(other.to_vec()));
        // looks without monitors tell nothing and forget nothing
        assert_eq!(watch.look(&[], 20.0), None);
        assert_eq!(watch.settled(), Some(other.as_slice()));
    }
}
