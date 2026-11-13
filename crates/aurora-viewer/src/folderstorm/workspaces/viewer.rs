//! The viewer window of a session: carrying out a layout's *Viewer window*
//! part, keeping its lock, and watching the monitors
//! (folderstorm/plans/workspaces.md sections 8.4 and 8.5; checkpoint 4 in
//! detail, points 4 and 6).
//!
//! What is wanted of the window between two applies comes from the store
//! alone: `Store::window_held` and the active layout's frame say *locked*
//! (not resizable) and *borderless*, or neither. Each frame [`frame`]
//! compares that with what it last asked of this window and sends the
//! difference. So unlocking is clearing `window_held`, a window made anew
//! (a GPU recovery) is locked again, and nothing is ever sent to a window
//! no layout holds.

use super::capture;
use super::monitors::{self, Watch};
use super::store::{Monitor, Store, WindowFrame};
use super::window::{Command, Hold, Plan, Seen, Steps};

/// Frames a window is given to settle after its steps, a start or a save,
/// before it is compared with for the "modified" mark.
const SETTLE: u64 = 12;
/// Frames between two looks at the window and at the monitors.
const CHECK_EVERY: u64 = 30;
/// A window moved or resized by more than this, in pixels, is modified
/// (section 8.3).
const MOVED: i32 = 4;

/// The window's place as the "modified" mark compares it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    inner: (u32, u32),
    outer: Option<(i32, i32)>,
    maximized: bool,
    fullscreen: bool,
}

impl Geometry {
    fn of(seen: &Seen) -> Self {
        Geometry {
            inner: seen.inner,
            outer: seen.outer,
            maximized: seen.maximized,
            fullscreen: seen.fullscreen,
        }
    }

    fn differs(&self, other: &Geometry) -> bool {
        let far = |a: i64, b: i64| (a - b).abs() > i64::from(MOVED);
        self.maximized != other.maximized
            || self.fullscreen != other.fullscreen
            || far(self.inner.0.into(), other.inner.0.into())
            || far(self.inner.1.into(), other.inner.1.into())
            || matches!((self.outer, other.outer), (Some(a), Some(b)) if far(a.0.into(), b.0.into()) || far(a.1.into(), b.1.into()))
    }
}

/// What the session knows of the viewer window.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// The window as the app last saw it; None while there is none to
    /// drive (a capture, a forced size, before the first frame).
    seen: Option<Seen>,
    /// A layout's window being put in place.
    running: Option<Steps>,
    /// What was last asked of the window `window`.
    hold: Hold,
    window: Option<u64>,
    /// Every monitor of the active layout is there: a lock may be given.
    complete: bool,
    /// The hold of the last session was looked at.
    started: bool,
    watch: Watch,
    /// The frame of the last look at the monitors and at the window.
    looked: u64,
    /// The frame from which the window has settled.
    settle: Option<u64>,
    /// Where the window was once settled.
    baseline: Option<Geometry>,
}

/// What a frame found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The store changed: the settings are to be saved.
    pub save: bool,
    /// The window's steps ended in this frame.
    pub placed: bool,
    /// The monitors changed, and this is the setup now.
    pub setup: Option<Vec<Monitor>>,
    /// The window was unlocked because a monitor of its layout is gone.
    pub unlocked: bool,
}

/// What `frame` asks of a window in force.
fn hold_of(frame: &WindowFrame, complete: bool) -> Hold {
    let resizable = !(frame.locked() && complete);
    match frame {
        WindowFrame::Normal { .. } => Hold {
            decorated: true,
            resizable,
        },
        WindowFrame::Borderless { .. } => Hold {
            decorated: false,
            resizable,
        },
        WindowFrame::Fullscreen { .. } => Hold::FREE,
    }
}

fn held_frame(store: &Store) -> Option<&WindowFrame> {
    store
        .window_held
        .then(|| store.active())
        .flatten()
        .and_then(|layout| layout.window.as_ref())
        .filter(|frame| frame.holds())
}

impl State {
    /// The window as the app last saw it.
    pub fn seen(&self) -> Option<&Seen> {
        self.seen.as_ref()
    }

    /// Whether a layout's window is still being put in place.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Starts putting the window where `plan` has it (None: nothing to
    /// do), in place of what was being done. `complete`: every monitor of
    /// the layout is there.
    pub fn begin(&mut self, plan: Option<Plan>, complete: bool) {
        self.running = plan.map(Steps::new);
        self.complete = complete;
        self.baseline = None;
        self.settle = None;
    }

    /// The window is as the active layout has it from now on: compared
    /// with a new baseline once settled.
    pub fn rebaseline(&mut self, now: u64) {
        self.baseline = None;
        self.settle = Some(now + SETTLE);
        self.complete = true;
    }

    /// One frame, before the interface's pass: `seen` is the window now.
    /// Carries on the steps of an apply, keeps the hold, looks at the
    /// monitors and at the window. What is asked of the window goes to
    /// `out`.
    pub fn frame(&mut self, ctx: &egui::Context, store: &mut Store, seen: Option<Seen>, out: &mut Vec<Command>) -> Found {
        let mut found = Found::default();
        self.seen = seen;
        let Some(seen) = seen else {
            // nothing to drive: a part asked of the window is dropped
            self.running = None;
            return found;
        };
        let now = ctx.cumulative_frame_nr();
        let t = ctx.input(|i| i.time);
        if self.window != Some(seen.id) {
            // a new window is Aurora's own: nothing asked of it yet
            self.hold = Hold::FREE;
            let renewed = self.window.replace(seen.id).is_some();
            if renewed && self.running.is_none() {
                self.started = false;
            }
        }
        if let Some(steps) = &mut self.running {
            if !steps.advance(&seen, t, out) {
                return found;
            }
            self.hold = steps.plan().hold;
            self.running = None;
            self.settle = Some(now + SETTLE);
            self.baseline = None;
            self.started = true;
            found.placed = true;
        }
        // the hold of the last session, or of the window before this one
        if !self.started && crate::folderstorm::diagram::desktop_known(ctx) {
            self.started = true;
            self.settle = Some(now + SETTLE);
            if let Some((frame, saved)) = store
                .window_held
                .then(|| store.active())
                .flatten()
                .and_then(|layout| Some((layout.window.clone()?, layout.monitors.clone())))
            {
                let asked = monitors::resolve(&frame, &saved, &capture::monitors(ctx), &seen);
                let plan = asked.plan.filter(|plan| asked.missing == 0 && plan.hold != Hold::FREE);
                self.complete = plan.is_some();
                match plan {
                    // put back over its monitors, without its border
                    Some(plan) if !plan.hold.decorated => {
                        self.running = Some(Steps::new(plan));
                        return found;
                    }
                    Some(_) => {}
                    None => {
                        store.window_held = false;
                        found.save = true;
                    }
                }
            }
        }
        // entering Aurora's full screen lets the window go
        if seen.fullscreen && store.window_held {
            store.window_held = false;
            found.save = true;
        }
        if now >= self.looked.saturating_add(CHECK_EVERY) {
            self.looked = now;
            if let Some(setup) = self.watch.look(&capture::monitors(ctx), t) {
                let saved = store.active().map(|layout| layout.monitors.as_slice()).unwrap_or_default();
                self.complete = monitors::missing(saved, &setup) == 0;
                if held_frame(store).is_some() && !self.complete {
                    store.window_held = false;
                    found.save = true;
                    found.unlocked = true;
                }
                found.setup = Some(setup);
                // what the system then does with the window is not the user
                self.baseline = None;
                self.settle = Some(now + SETTLE);
            } else if self.settle.is_none()
                && !seen.minimized
                && !store.modified
                && store.active().is_some_and(|layout| layout.window.is_some())
                && self.baseline.is_some_and(|baseline| baseline.differs(&Geometry::of(&seen)))
            {
                store.modified = true;
                found.save = true;
            }
        }
        if self.settle.is_some_and(|at| now >= at) && !seen.minimized {
            self.settle = None;
            self.baseline = Some(Geometry::of(&seen));
        }
        let wanted = held_frame(store).map_or(Hold::FREE, |frame| hold_of(frame, self.complete));
        if wanted != self.hold {
            self.hold.to(wanted, out);
            self.hold = wanted;
        }
        found
    }
}
