//! Hibernation in the viewer (`folderstorm/plans/deep-sleep.md`, P2): when
//! a background character's viewer lets go of its whole renderer, and when
//! it makes a new one. All pure; the viewer's side is
//! `app/folderstorm_characters/sleep.rs`.
//!
//! - **When to sleep** ([`Grace::decide`]): the host asks for Hibernation
//!   (`ipc::Memory::hibernate`), Economy's release minimised the window and
//!   nobody has gone to it since, it is not the active character and has
//!   not the keyboard. The host's grace ([`crate::memory::DEFAULT_HIBERNATE_AFTER_S`])
//!   counts from then: a quick flip back costs only an Economy return, and
//!   the outgoing character's teardown does not overlap the incoming one's
//!   wake. Past the grace it still stays in Economy ([`Stay`]) while it is
//!   not in the world or arrived less than [`MIN_IN_WORLD`] ago, holds the
//!   voice, is heard, has VR set up, has no CPU copy of egui's textures
//!   (`AURORA_GPU_RECOVERY=0`) or takes a photo.
//! - **When to wake** ([`wake`]): it becomes the active character, a
//!   monitor on screen watches it, the host says Show, the window is closed
//!   (it wakes to ask, then quits), or the user restores or focuses the
//!   window. A change of mind while asleep (Warm or Economy chosen, the host
//!   gone) wakes nothing: a minimised window draws nothing either way, and
//!   the next restore wakes it.
//! - **How**: the window first ([`Wake::restores`]), then the renderer once
//!   the window has a size ([`can_build`]: a minimised window's surface is
//!   0 × 0), retried every [`RETRY`] ([`Rebuild`]).
//! - **While asleep** the viewer runs the first half of its frame every
//!   [`TICK`] ([`Pacer`]): network, chat, messages, the local assistant.

use std::time::{Duration, Instant};

/// While asleep, the viewer's frame (its network, chat and assistant half)
/// runs this often, and the event loop waits in between.
pub const TICK: Duration = Duration::from_millis(200);
/// A renderer that could not be made is tried again after this long.
pub const RETRY: Duration = Duration::from_secs(2);
/// Failures in a row before the active character gives up (Aurora's
/// failed-recovery path).
pub const ACTIVE_ATTEMPTS: u32 = 3;
/// A character just arrived stays in Economy this long: its world is still
/// coming in, and a new arrival is often looked at again.
pub const MIN_IN_WORLD: Duration = Duration::from_secs(30);

/// What the viewer knows of itself at a turn of its event loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The host asks for Hibernation.
    pub hibernate: bool,
    /// The host's grace.
    pub after: Duration,
    /// Economy's release minimised the window and nobody went to it since.
    pub minimised_by_host: bool,
    /// The window is minimised now.
    pub minimised: bool,
    pub keyboard: bool,
    /// The active character.
    pub active: bool,
    /// Economy's release is under way (the window is not minimised yet).
    pub releasing: bool,
    /// How long it has been in the world (None: not in the world).
    pub in_world_for: Option<Duration>,
    /// It holds a voice connection.
    pub voice: bool,
    /// Its sounds are heard (the host does not cut them).
    pub heard: bool,
    /// VR was set up for this run.
    pub vr: bool,
    /// egui's textures have no CPU copy to send again to a new renderer.
    pub no_egui_copy: bool,
    /// A photo is under way.
    pub photo: bool,
}

/// Why a character past its grace stays in Economy for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stay {
    NotInWorld,
    JustArrived,
    Voice,
    Heard,
    Vr,
    NoEguiCopy,
    Photo,
}

impl Stay {
    /// For the log.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotInWorld => "not in the world",
            Self::JustArrived => "arrived in the world less than 30 s ago",
            Self::Voice => "it holds the voice",
            Self::Heard => "it is heard in the background",
            Self::Vr => "VR is set up",
            Self::NoEguiCopy => "AURORA_GPU_RECOVERY=0 keeps no copy of the interface's textures",
            Self::Photo => "a photo is under way",
        }
    }
}

/// What to do about hibernating, at this turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Not a case for it (not asked, not minimised by the host, in front…).
    Idle,
    /// Minimised, the grace running: `left` to go.
    Grace { left: Duration },
    /// Past the grace, kept in Economy for now.
    Stay(Stay),
    /// Let the renderer go now.
    Sleep,
}

/// The grace before hibernating: when the window became a case for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grace {
    since: Option<Instant>,
}

impl Grace {
    /// Start over (it slept, or woke).
    pub fn reset(&mut self) {
        self.since = None;
    }

    /// How long it has been a case for hibernating (zero: not one).
    pub fn waited(&self, now: Instant) -> Duration {
        self.since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    /// At each turn while awake.
    pub fn decide(&mut self, f: &Facts, now: Instant) -> Decision {
        let case = f.hibernate && f.minimised_by_host && f.minimised && !f.keyboard && !f.active && !f.releasing;
        if !case {
            self.since = None;
            return Decision::Idle;
        }
        let since = *self.since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        if waited < f.after {
            return Decision::Grace { left: f.after - waited };
        }
        let stay = match f.in_world_for {
            None => Some(Stay::NotInWorld),
            Some(d) if d < MIN_IN_WORLD => Some(Stay::JustArrived),
            Some(_) if f.voice => Some(Stay::Voice),
            Some(_) if f.heard => Some(Stay::Heard),
            Some(_) if f.vr => Some(Stay::Vr),
            Some(_) if f.no_egui_copy => Some(Stay::NoEguiCopy),
            Some(_) if f.photo => Some(Stay::Photo),
            Some(_) => None,
        };
        stay.map_or(Decision::Sleep, Decision::Stay)
    }
}

/// What a sleeping viewer sees at a turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WakeFacts {
    /// The host made it the active character.
    pub active: bool,
    /// A monitor on screen watches it (Economy's lowered release).
    pub watched: bool,
    /// The host asked for its window (Show).
    pub show: bool,
    /// Its window was asked to close.
    pub closing: bool,
    /// The window has the keyboard.
    pub focused: bool,
    /// The window is not minimised any more and has a size.
    pub restored: bool,
}

/// Why a sleeping viewer wakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    Active,
    Show,
    Watched,
    Closing,
    Focused,
    Restored,
}

/// What a wake does with the window before the renderer is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restore {
    /// Restored and brought forward with the keyboard.
    Front,
    /// Restored behind the active window, without the keyboard.
    Behind,
    /// The user already restored it.
    Nothing,
}

impl Wake {
    pub fn restores(self) -> Restore {
        match self {
            Self::Active | Self::Show | Self::Closing => Restore::Front,
            Self::Watched => Restore::Behind,
            Self::Focused | Self::Restored => Restore::Nothing,
        }
    }

    /// For the log.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Active => "it is the active character",
            Self::Show => "the host shows it",
            Self::Watched => "a monitor shows it",
            Self::Closing => "its window is closed",
            Self::Focused => "its window has the keyboard",
            Self::Restored => "its window was restored",
        }
    }
}

/// Whether a sleeping viewer wakes now, and why (the first reason that
/// holds).
pub fn wake(f: &WakeFacts) -> Option<Wake> {
    if f.closing {
        Some(Wake::Closing)
    } else if f.show {
        Some(Wake::Show)
    } else if f.active {
        Some(Wake::Active)
    } else if f.watched {
        Some(Wake::Watched)
    } else if f.focused {
        Some(Wake::Focused)
    } else if f.restored {
        Some(Wake::Restored)
    } else {
        None
    }
}

/// Whether a renderer can be made for the window now: it has a size and
/// is not minimised (a Vulkan surface on a minimised Windows window is
/// 0 × 0). `minimised`: None where the system does not say.
pub fn can_build(inner: (u32, u32), minimised: Option<bool>) -> bool {
    inner.0 > 0 && inner.1 > 0 && minimised != Some(true)
}

/// What to do after a renderer could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retry {
    /// Again after [`RETRY`].
    Again,
    /// The active character gives up: Aurora's failed-recovery path.
    GiveUp,
    /// Behind, it stays asleep until the next reason to wake.
    StayAsleep,
}

/// The renderer's attempts during a wake.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rebuild {
    failures: u32,
    next: Option<Instant>,
}

impl Rebuild {
    /// A new wake: nothing failed yet.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether an attempt may be made now.
    pub fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|at| now >= at)
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// An attempt failed; `active`: the active character's wake.
    pub fn failed(&mut self, now: Instant, active: bool) -> Retry {
        self.failures += 1;
        self.next = Some(now + RETRY);
        if self.failures < ACTIVE_ATTEMPTS {
            Retry::Again
        } else if active {
            Retry::GiveUp
        } else {
            Retry::StayAsleep
        }
    }
}

/// The frames of a sleeping viewer, every [`TICK`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pacer {
    last: Option<Instant>,
}

impl Pacer {
    /// Whether a frame is due now (taken: the next one after [`TICK`]).
    pub fn due(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|last| now.saturating_duration_since(last) < TICK) {
            return false;
        }
        self.last = Some(now);
        true
    }

    /// When the event loop wakes up next.
    pub fn next(&self, now: Instant) -> Instant {
        self.last.map_or(now, |last| last + TICK).max(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    /// Minimised by Economy's release, asked to hibernate, in the world
    /// for a while: a case for it.
    fn case() -> Facts {
        Facts {
            hibernate: true,
            after: 20 * S,
            minimised_by_host: true,
            minimised: true,
            in_world_for: Some(120 * S),
            ..Facts::default()
        }
    }

    #[test]
    fn it_sleeps_once_minimised_for_the_grace() {
        let t0 = Instant::now();
        let mut g = Grace::default();
        assert_eq!(g.decide(&case(), t0), Decision::Grace { left: 20 * S });
        assert_eq!(g.decide(&case(), t0 + 5 * S), Decision::Grace { left: 15 * S });
        assert_eq!(g.waited(t0 + 5 * S), 5 * S);
        assert_eq!(g.decide(&case(), t0 + 20 * S), Decision::Sleep);
        // the grace is the host's
        let mut quick = Grace::default();
        let short = Facts { after: 5 * S, ..case() };
        assert_eq!(quick.decide(&short, t0), Decision::Grace { left: 5 * S });
        assert_eq!(quick.decide(&short, t0 + 5 * S), Decision::Sleep);
    }

    #[test]
    fn the_grace_starts_over_whenever_it_is_no_case_for_it() {
        let t0 = Instant::now();
        for broken in [
            Facts {
                hibernate: false,
                ..case()
            },
            Facts {
                minimised_by_host: false,
                ..case()
            },
            Facts {
                minimised: false,
                ..case()
            },
            Facts { keyboard: true, ..case() },
            Facts { active: true, ..case() },
            Facts { releasing: true, ..case() },
        ] {
            let mut g = Grace::default();
            assert!(matches!(g.decide(&case(), t0), Decision::Grace { .. }));
            assert_eq!(g.decide(&broken, t0 + 10 * S), Decision::Idle, "{broken:?}");
            assert_eq!(g.waited(t0 + 10 * S), Duration::ZERO);
            // a case again: the whole grace from there
            assert_eq!(g.decide(&case(), t0 + 15 * S), Decision::Grace { left: 20 * S }, "{broken:?}");
            assert_eq!(g.decide(&case(), t0 + 34 * S), Decision::Grace { left: S });
            assert_eq!(g.decide(&case(), t0 + 35 * S), Decision::Sleep);
        }
    }

    #[test]
    fn past_the_grace_the_fallbacks_keep_it_in_economy() {
        let t0 = Instant::now();
        let late = t0 + 60 * S;
        for (facts, stay) in [
            (
                Facts {
                    in_world_for: None,
                    ..case()
                },
                Stay::NotInWorld,
            ),
            (
                Facts {
                    in_world_for: Some(10 * S),
                    ..case()
                },
                Stay::JustArrived,
            ),
            (Facts { voice: true, ..case() }, Stay::Voice),
            (Facts { heard: true, ..case() }, Stay::Heard),
            (Facts { vr: true, ..case() }, Stay::Vr),
            (
                Facts {
                    no_egui_copy: true,
                    ..case()
                },
                Stay::NoEguiCopy,
            ),
            (Facts { photo: true, ..case() }, Stay::Photo),
        ] {
            let mut g = Grace::default();
            g.decide(&facts, t0);
            assert_eq!(g.decide(&facts, late), Decision::Stay(stay), "{facts:?}");
            assert!(!stay.reason().is_empty());
            // once it no longer holds: asleep at once, the grace is spent
            assert_eq!(g.decide(&case(), late + S), Decision::Sleep, "{facts:?}");
        }
        // just arrived: asleep once 30 s in the world
        let mut g = Grace::default();
        let arrived = |d| Facts {
            in_world_for: Some(d),
            ..case()
        };
        g.decide(&arrived(S), t0);
        assert_eq!(g.decide(&arrived(21 * S), t0 + 20 * S), Decision::Stay(Stay::JustArrived));
        assert_eq!(g.decide(&arrived(MIN_IN_WORLD), t0 + 29 * S), Decision::Sleep);
    }

    #[test]
    fn each_reason_wakes_it_and_says_what_becomes_of_the_window() {
        assert_eq!(wake(&WakeFacts::default()), None, "nothing: it sleeps on");
        let cases = [
            (
                WakeFacts {
                    active: true,
                    ..Default::default()
                },
                Wake::Active,
                Restore::Front,
            ),
            (
                WakeFacts {
                    show: true,
                    ..Default::default()
                },
                Wake::Show,
                Restore::Front,
            ),
            (
                WakeFacts {
                    watched: true,
                    ..Default::default()
                },
                Wake::Watched,
                Restore::Behind,
            ),
            (
                WakeFacts {
                    closing: true,
                    ..Default::default()
                },
                Wake::Closing,
                Restore::Front,
            ),
            (
                WakeFacts {
                    focused: true,
                    ..Default::default()
                },
                Wake::Focused,
                Restore::Nothing,
            ),
            (
                WakeFacts {
                    restored: true,
                    ..Default::default()
                },
                Wake::Restored,
                Restore::Nothing,
            ),
        ];
        for (facts, why, window) in cases {
            assert_eq!(wake(&facts), Some(why), "{facts:?}");
            assert_eq!(why.restores(), window, "{why:?}");
            assert!(!why.reason().is_empty());
        }
        // the host's word goes first: shown in front, not merely restored
        let both = WakeFacts {
            show: true,
            restored: true,
            watched: true,
            ..Default::default()
        };
        assert_eq!(wake(&both), Some(Wake::Show));
        // the active character in front even while a monitor watched it
        let both = WakeFacts {
            active: true,
            watched: true,
            ..Default::default()
        };
        assert_eq!(wake(&both), Some(Wake::Active));
    }

    #[test]
    fn the_renderer_waits_for_a_window_with_a_size() {
        assert!(!can_build((0, 0), Some(true)), "minimised: 0 × 0");
        assert!(!can_build((1920, 1080), Some(true)), "still minimised");
        assert!(!can_build((0, 1080), Some(false)));
        assert!(!can_build((1920, 0), None));
        assert!(can_build((1920, 1080), Some(false)));
        assert!(can_build((800, 560), None), "the system does not say: by its size");
    }

    #[test]
    fn a_renderer_that_fails_is_tried_again_and_the_active_one_gives_up() {
        let t0 = Instant::now();
        let mut r = Rebuild::default();
        assert!(r.due(t0));
        assert_eq!(r.failed(t0, true), Retry::Again);
        assert!(!r.due(t0 + S));
        assert!(r.due(t0 + RETRY));
        assert_eq!(r.failed(t0 + RETRY, true), Retry::Again);
        assert_eq!(r.failed(t0 + RETRY * 2, true), Retry::GiveUp);
        assert_eq!(r.failures(), ACTIVE_ATTEMPTS);
        // behind: it stays asleep instead
        let mut behind = Rebuild::default();
        for _ in 1..ACTIVE_ATTEMPTS {
            assert_eq!(behind.failed(t0, false), Retry::Again);
        }
        assert_eq!(behind.failed(t0, false), Retry::StayAsleep);
        behind.reset();
        assert!(behind.due(t0));
        assert_eq!(behind.failures(), 0);
    }

    #[test]
    fn a_sleeping_viewer_runs_its_frame_every_tick() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut p = Pacer::default();
        assert_eq!(p.next(t0), t0, "none yet: at once");
        assert!(p.due(t0));
        assert!(!p.due(t0 + ms(50)));
        assert_eq!(p.next(t0 + ms(50)), t0 + TICK);
        assert!(!p.due(t0 + ms(199)));
        assert!(p.due(t0 + TICK));
        // late: at once, never in the past
        assert_eq!(p.next(t0 + ms(900)), t0 + ms(900));
        assert!(p.due(t0 + ms(900)));
        assert_eq!(TICK, ms(200));
    }
}
