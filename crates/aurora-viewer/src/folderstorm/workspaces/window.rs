//! The viewer window as a layout sees and drives it, without winit
//! (folderstorm/plans/workspaces.md, checkpoint 4 in detail, point 3).
//!
//! Each frame the app says what the window is like ([`Seen`]) and runs the
//! [`Command`]s it gets back (`app/folderstorm_workspaces.rs`). Putting a
//! window somewhere takes several frames: the system answers a request
//! when it likes, and a size asked of a maximized or full-screen window is
//! lost. [`Steps`] is that sequence, each step waiting for the window to
//! say the one before is done, or for its time to run out.

/// The viewer window this frame, in desktop pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    /// Which window it is: another number after the window was made anew
    /// (a GPU recovery).
    pub id: u64,
    /// Its inside.
    pub inner: (u32, u32),
    /// Its outer corner; None where the system does not tell (Wayland).
    pub outer: Option<(i32, i32)>,
    /// Pixels per point of the window.
    pub scale: f32,
    pub maximized: bool,
    /// Aurora's full screen.
    pub fullscreen: bool,
    pub minimized: bool,
    /// It has its border and title bar.
    pub decorated: bool,
    pub resizable: bool,
    /// The outer corner and inside it goes back to when it is maximized or
    /// in full screen, as Aurora remembers them.
    pub normal: Option<[i32; 4]>,
}

impl Seen {
    /// Its rectangle on the desktop (x, y, width, height), when the system
    /// tells where it is.
    pub fn rect(&self) -> Option<[i32; 4]> {
        let (x, y) = self.outer?;
        Some([x, y, to_i32(self.inner.0), to_i32(self.inner.1)])
    }
}

pub(super) fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// One thing asked of the viewer window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Aurora's full screen on or off, through its own setting.
    Fullscreen(bool),
    Maximized(bool),
    /// The border and title bar.
    Decorations(bool),
    Resizable(bool),
    /// The outer corner.
    Position(i32, i32),
    /// The inside.
    Size(u32, u32),
}

/// What holds the window between two layouts: its border and whether the
/// user can resize it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    pub decorated: bool,
    pub resizable: bool,
}

impl Hold {
    /// Aurora's own window: nothing held.
    pub const FREE: Hold = Hold {
        decorated: true,
        resizable: true,
    };

    /// The commands that take the window from `self` to `to`.
    pub fn to(self, to: Hold, out: &mut Vec<Command>) {
        if self.decorated != to.decorated {
            out.push(Command::Decorations(to.decorated));
        }
        if self.resizable != to.resizable {
            out.push(Command::Resizable(to.resizable));
        }
    }
}

impl Default for Hold {
    fn default() -> Self {
        Hold::FREE
    }
}

/// Where the window is asked to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// A window with its border: its outer corner (None: where it is), its
    /// inside, then maximized or not.
    Normal {
        at: Option<(i32, i32)>,
        size: (u32, u32),
        maximized: bool,
    },
    /// Aurora's full screen, on the monitor holding the point `on` (None:
    /// the monitor the window is on).
    Fullscreen { on: Option<(i32, i32)> },
    /// No border, exactly over this rectangle (x, y, width, height).
    Borderless { rect: [i32; 4] },
}

/// A goal and what holds the window once it is there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub goal: Goal,
    pub hold: Hold,
}

/// Seconds the window is given to leave full screen.
const LEAVE: f64 = 1.5;
/// Seconds it is given to take or lose its border.
const BORDER: f64 = 0.5;
/// Seconds it is given to take its size (section 8.2).
const SIZE: f64 = 1.5;
/// Frames without change after which it is taken to have settled.
const STILL: u32 = 10;
/// Seconds before a window that has not changed at all is taken to have
/// settled: until then it may not have heard the request yet.
const UNHEARD: f64 = 0.5;
/// A size or a place this close to the one asked, in pixels, is the one.
const NEAR: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Begin,
    /// Asked to leave full screen at `since`.
    Windowed {
        since: f64,
    },
    /// Asked to leave maximized and to take the goal's border at `since`.
    Border {
        since: f64,
    },
    /// Asked for its place and size at `since`; `last` is what it was like
    /// in the frame before, `still` for how many frames, `changed` whether
    /// it has changed at all since.
    Sized {
        since: f64,
        last: ((u32, u32), Option<(i32, i32)>),
        still: u32,
        changed: bool,
    },
    Done,
}

/// A plan being carried out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    plan: Plan,
    stage: Stage,
}

impl Steps {
    pub fn new(plan: Plan) -> Self {
        Steps { plan, stage: Stage::Begin }
    }

    pub fn plan(&self) -> Plan {
        self.plan
    }

    fn decorated(&self) -> bool {
        !matches!(self.plan.goal, Goal::Borderless { .. })
    }

    /// One frame: `seen` is the window now, `t` the time in seconds. What
    /// is asked of the window goes to `out`. True once the plan is carried
    /// out (the window then has the plan's hold).
    pub fn advance(&mut self, seen: &Seen, t: f64, out: &mut Vec<Command>) -> bool {
        // (a loop: a step whose wait is already over goes on to the next
        // in the same frame)
        loop {
            match self.stage {
                Stage::Begin => {
                    // programmatic sizes and a maximize need a window that
                    // can change size
                    if !seen.resizable {
                        out.push(Command::Resizable(true));
                    }
                    match self.plan.goal {
                        // already in full screen, and where it is will do
                        Goal::Fullscreen { on: None } if seen.fullscreen => {
                            self.plan.hold.to_from_unknown(seen, out);
                            self.stage = Stage::Done;
                        }
                        _ => {
                            if seen.fullscreen {
                                out.push(Command::Fullscreen(false));
                            }
                            self.stage = Stage::Windowed { since: t };
                        }
                    }
                }
                Stage::Windowed { since } => {
                    if seen.fullscreen && t - since < LEAVE {
                        return false;
                    }
                    if seen.maximized {
                        out.push(Command::Maximized(false));
                    }
                    if seen.decorated != self.decorated() {
                        out.push(Command::Decorations(self.decorated()));
                    }
                    self.stage = Stage::Border { since: t };
                }
                Stage::Border { since } => {
                    let ready = seen.decorated == self.decorated() && !seen.maximized;
                    if !(ready || t - since >= BORDER) {
                        return false;
                    }
                    match self.plan.goal {
                        Goal::Normal { at, size, .. } => {
                            if let Some((x, y)) = at {
                                out.push(Command::Position(x, y));
                            }
                            out.push(Command::Size(size.0, size.1));
                        }
                        Goal::Borderless {
                            rect: [x, y, width, height],
                        } => {
                            out.push(Command::Position(x, y));
                            out.push(Command::Size(width.max(1) as u32, height.max(1) as u32));
                        }
                        Goal::Fullscreen { on } => {
                            if let Some((x, y)) = on {
                                out.push(Command::Position(x, y));
                            }
                        }
                    }
                    self.stage = Stage::Sized {
                        since: t,
                        last: (seen.inner, seen.outer),
                        still: 0,
                        changed: false,
                    };
                    return false;
                }
                Stage::Sized {
                    since,
                    last,
                    still,
                    changed,
                } => {
                    let now = (seen.inner, seen.outer);
                    let changed = changed || now != last;
                    let still = if now == last { still + 1 } else { 0 };
                    // settled somewhere else (the system cut the size), or
                    // deaf to the request
                    let settled = still >= STILL && (changed || t - since >= UNHEARD);
                    if !(self.arrived(seen) || settled || t - since >= SIZE) {
                        self.stage = Stage::Sized {
                            since,
                            last: now,
                            still,
                            changed,
                        };
                        return false;
                    }
                    match self.plan.goal {
                        Goal::Normal { maximized: true, .. } => out.push(Command::Maximized(true)),
                        Goal::Fullscreen { .. } => out.push(Command::Fullscreen(true)),
                        _ => {}
                    }
                    if !self.plan.hold.resizable {
                        out.push(Command::Resizable(false));
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => return true,
            }
        }
    }

    /// The window is where the goal puts it.
    fn arrived(&self, seen: &Seen) -> bool {
        let near = |a: i32, b: i32| (a - b).abs() <= NEAR;
        let sized = |size: (u32, u32)| near(to_i32(seen.inner.0), to_i32(size.0)) && near(to_i32(seen.inner.1), to_i32(size.1));
        let placed = |at: Option<(i32, i32)>| match (at, seen.outer) {
            (Some((x, y)), Some((sx, sy))) => near(x, sx) && near(y, sy),
            _ => true,
        };
        match self.plan.goal {
            Goal::Normal { at, size, .. } => sized(size) && placed(at),
            Goal::Borderless {
                rect: [x, y, width, height],
            } => sized((width.max(1) as u32, height.max(1) as u32)) && placed(Some((x, y))),
            Goal::Fullscreen { on } => placed(on),
        }
    }
}

impl Hold {
    /// The commands that give a window seen as `seen` this hold, when what
    /// was last asked of it is not known.
    fn to_from_unknown(self, seen: &Seen, out: &mut Vec<Command>) {
        if seen.decorated != self.decorated {
            out.push(Command::Decorations(self.decorated));
        }
        if !self.resizable {
            out.push(Command::Resizable(false));
        }
    }
}

/// A pretend viewer window for the tests: it runs the commands as a
/// system would, at once or some frames late.
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct Pretend {
    pub seen: Seen,
    /// Frames before a command takes effect.
    pub lag: u32,
    /// Every command it was sent, in order.
    pub log: Vec<Command>,
    waiting: Vec<(u32, Command)>,
    /// Sizes are ignored (a window manager that refuses).
    pub stuck: bool,
    /// The largest inside the system gives (Windows cuts a window to the
    /// desktop).
    pub largest: Option<(u32, u32)>,
}

#[cfg(test)]
impl Pretend {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Pretend {
            seen: Seen {
                id: 1,
                inner: (width, height),
                outer: Some((x, y)),
                scale: 1.0,
                maximized: false,
                fullscreen: false,
                minimized: false,
                decorated: true,
                resizable: true,
                normal: None,
            },
            lag: 0,
            log: Vec::new(),
            waiting: Vec::new(),
            stuck: false,
            largest: None,
        }
    }

    pub fn send(&mut self, commands: &[Command]) {
        for command in commands {
            self.log.push(*command);
            self.waiting.push((self.lag, *command));
        }
        self.tick(false);
    }

    /// The end of a frame: commands whose time has come take effect.
    pub fn frame(&mut self) {
        self.tick(true);
    }

    fn tick(&mut self, age: bool) {
        let mut later = Vec::new();
        for (wait, command) in std::mem::take(&mut self.waiting) {
            if wait > 0 {
                later.push((if age { wait - 1 } else { wait }, command));
                continue;
            }
            let seen = &mut self.seen;
            match command {
                Command::Fullscreen(on) => {
                    if on && !seen.fullscreen {
                        seen.normal = seen.rect();
                    }
                    if !on
                        && seen.fullscreen
                        && let Some([x, y, width, height]) = seen.normal.take()
                    {
                        seen.outer = Some((x, y));
                        seen.inner = (width as u32, height as u32);
                    }
                    seen.fullscreen = on;
                }
                Command::Maximized(on) => seen.maximized = on,
                Command::Decorations(on) => seen.decorated = on,
                Command::Resizable(on) => seen.resizable = on,
                Command::Position(x, y) => {
                    if seen.outer.is_some() {
                        seen.outer = Some((x, y));
                    }
                }
                Command::Size(width, height) => {
                    if !self.stuck {
                        let (most_w, most_h) = self.largest.unwrap_or((u32::MAX, u32::MAX));
                        seen.inner = (width.min(most_w), height.min(most_h));
                    }
                }
            }
        }
        self.waiting = later;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKED: Hold = Hold {
        decorated: true,
        resizable: false,
    };

    /// Runs `steps` against `window` at 60 frames a second; how many
    /// frames it took.
    fn run(steps: &mut Steps, window: &mut Pretend) -> u32 {
        for frame in 0..600 {
            let mut out = Vec::new();
            let done = steps.advance(&window.seen, f64::from(frame) / 60.0, &mut out);
            window.send(&out);
            window.frame();
            if done {
                // what was asked last takes effect
                for _ in 0..=window.lag.min(60) {
                    window.frame();
                }
                return frame;
            }
        }
        panic!("the steps never end: {steps:?}");
    }

    fn normal(x: i32, y: i32, width: u32, height: u32) -> Goal {
        Goal::Normal {
            at: Some((x, y)),
            size: (width, height),
            maximized: false,
        }
    }

    #[test]
    fn a_window_takes_its_place_and_size_then_its_lock() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        let mut steps = Steps::new(Plan {
            goal: normal(0, 0, 3840, 1040),
            hold: LOCKED,
        });
        let frames = run(&mut steps, &mut window);
        assert_eq!(window.seen.outer, Some((0, 0)));
        assert_eq!(window.seen.inner, (3840, 1040));
        assert!(!window.seen.resizable && window.seen.decorated);
        assert_eq!(
            window.log,
            [Command::Position(0, 0), Command::Size(3840, 1040), Command::Resizable(false)],
            "nothing more than what changes"
        );
        assert!(frames <= 3, "a window that answers at once is done at once: {frames}");
    }

    #[test]
    fn a_locked_window_is_unlocked_before_it_is_resized() {
        let mut window = Pretend::new(0, 0, 3840, 1040);
        window.seen.resizable = false;
        let mut steps = Steps::new(Plan {
            goal: normal(0, 0, 1920, 1040),
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        assert_eq!(window.log[0], Command::Resizable(true));
        assert!(window.seen.resizable);
        assert_eq!(window.seen.inner, (1920, 1040));
    }

    #[test]
    fn full_screen_and_maximized_are_left_before_the_size_is_asked() {
        let mut window = Pretend::new(0, 0, 1920, 1080);
        window.seen.fullscreen = true;
        window.seen.normal = Some([200, 100, 1280, 720]);
        window.lag = 3;
        let mut steps = Steps::new(Plan {
            goal: normal(10, 20, 1600, 900),
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        let leave = window.log.iter().position(|c| *c == Command::Fullscreen(false)).expect("left");
        let size = window.log.iter().position(|c| *c == Command::Size(1600, 900)).expect("sized");
        assert!(leave < size);
        assert!(!window.seen.fullscreen);
        assert_eq!((window.seen.outer, window.seen.inner), (Some((10, 20)), (1600, 900)));

        let mut window = Pretend::new(0, 0, 1920, 1040);
        window.seen.maximized = true;
        let mut steps = Steps::new(Plan {
            goal: Goal::Normal {
                at: Some((10, 20)),
                size: (1600, 900),
                maximized: true,
            },
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        assert_eq!(
            window.log,
            [
                Command::Maximized(false),
                Command::Position(10, 20),
                Command::Size(1600, 900),
                Command::Maximized(true)
            ]
        );
    }

    #[test]
    fn the_size_is_waited_for_until_the_window_reports_it() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        window.lag = 20;
        let mut steps = Steps::new(Plan {
            goal: normal(0, 0, 3840, 1040),
            hold: LOCKED,
        });
        let frames = run(&mut steps, &mut window);
        // (ten frames without change are not the end while the window has
        // not moved at all: the lock comes once the size is there)
        assert!(frames >= 20, "{frames}");
        assert_eq!(window.seen.inner, (3840, 1040));
        assert!(!window.seen.resizable);
    }

    #[test]
    fn a_window_that_never_takes_its_size_does_not_hold_the_apply() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        window.stuck = true;
        let mut steps = Steps::new(Plan {
            goal: normal(100, 100, 3840, 1040),
            hold: LOCKED,
        });
        let frames = run(&mut steps, &mut window);
        // half a second without any change, well before the second and a half
        assert!((30..=34).contains(&frames), "{frames}");
        assert_eq!(window.seen.inner, (1280, 720));
        assert!(!window.seen.resizable, "the lock is still given");
    }

    #[test]
    fn a_size_the_system_cuts_is_settled_after_ten_still_frames() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        window.largest = Some((1936, 1056));
        let mut steps = Steps::new(Plan {
            goal: normal(100, 100, 3840, 1040),
            hold: Hold::FREE,
        });
        let frames = run(&mut steps, &mut window);
        assert!((STILL..=STILL + 3).contains(&frames), "{frames}");
        assert_eq!(window.seen.inner, (1936, 1040));
    }

    #[test]
    fn a_window_still_in_full_screen_after_its_time_is_asked_anyway() {
        let mut window = Pretend::new(0, 0, 1920, 1080);
        window.seen.fullscreen = true;
        window.lag = 10_000;
        let mut steps = Steps::new(Plan {
            goal: normal(0, 0, 1600, 900),
            hold: Hold::FREE,
        });
        let frames = run(&mut steps, &mut window);
        let most = ((LEAVE + BORDER + SIZE) * 60.0) as u32 + 6;
        assert!(frames <= most, "{frames}");
        assert!(window.log.contains(&Command::Size(1600, 900)));
    }

    #[test]
    fn borderless_drops_the_border_then_covers_the_rectangle() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        window.lag = 2;
        let mut steps = Steps::new(Plan {
            goal: Goal::Borderless { rect: [0, 0, 3840, 1080] },
            hold: Hold {
                decorated: false,
                resizable: false,
            },
        });
        run(&mut steps, &mut window);
        assert_eq!(
            window.log,
            [
                Command::Decorations(false),
                Command::Position(0, 0),
                Command::Size(3840, 1080),
                Command::Resizable(false)
            ]
        );
        assert_eq!((window.seen.outer, window.seen.inner), (Some((0, 0)), (3840, 1080)));
        assert!(!window.seen.decorated);
        // and back to a window with its border
        let mut steps = Steps::new(Plan {
            goal: normal(50, 60, 1280, 720),
            hold: Hold::FREE,
        });
        window.log.clear();
        run(&mut steps, &mut window);
        assert_eq!(
            window.log,
            [
                Command::Resizable(true),
                Command::Decorations(true),
                Command::Position(50, 60),
                Command::Size(1280, 720)
            ]
        );
    }

    #[test]
    fn full_screen_goes_to_its_monitor_first() {
        let mut window = Pretend::new(100, 100, 1280, 720);
        let mut steps = Steps::new(Plan {
            goal: Goal::Fullscreen { on: Some((2400, 270)) },
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        assert_eq!(window.log, [Command::Position(2400, 270), Command::Fullscreen(true)]);
        // already in full screen where it is: nothing is asked
        let mut steps = Steps::new(Plan {
            goal: Goal::Fullscreen { on: None },
            hold: Hold::FREE,
        });
        window.log.clear();
        assert_eq!(run(&mut steps, &mut window), 0);
        assert!(window.log.is_empty());
        // on another monitor: out, over, in again
        let mut steps = Steps::new(Plan {
            goal: Goal::Fullscreen { on: Some((480, 270)) },
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        assert_eq!(
            window.log,
            [Command::Fullscreen(false), Command::Position(480, 270), Command::Fullscreen(true)]
        );
    }

    #[test]
    fn a_window_without_a_place_is_only_sized() {
        // Wayland: no position to read or to set
        let mut window = Pretend::new(0, 0, 1280, 720);
        window.seen.outer = None;
        let mut steps = Steps::new(Plan {
            goal: Goal::Normal {
                at: None,
                size: (1600, 900),
                maximized: false,
            },
            hold: Hold::FREE,
        });
        run(&mut steps, &mut window);
        assert_eq!(window.log, [Command::Size(1600, 900)]);
    }

    #[test]
    fn a_hold_changes_only_what_differs() {
        let mut out = Vec::new();
        Hold::FREE.to(LOCKED, &mut out);
        assert_eq!(out, [Command::Resizable(false)]);
        out.clear();
        LOCKED.to(
            Hold {
                decorated: false,
                resizable: false,
            },
            &mut out,
        );
        assert_eq!(out, [Command::Decorations(false)]);
        out.clear();
        LOCKED.to(LOCKED, &mut out);
        assert!(out.is_empty());
    }
}
