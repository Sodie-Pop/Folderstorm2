//! Checkpoint 9 of the session host: each monitor in a window of its own,
//! and the cycling monitor (session-host plan, checkpoint 9).
//!
//! The rules, all pure:
//! - what is remembered of a monitor's window ([`Window`]: its place, its
//!   pin, its chat part) and of the monitors together ([`Config`]: the
//!   size and rate a new monitor opens with, the cycling monitor),
//!   checked when the settings file is read ([`Config::cleaned`]);
//! - **a monitor's window follows the character that is not in front**
//!   ([`follow`]): when its character comes to the front, it takes the
//!   one that left the front;
//! - a character shown by two windows is asked once, for the larger size
//!   and the faster rate ([`cover`]);
//! - **the cycling monitor** ([`Cycle`]): each background character in
//!   turn, for an interval; the turn does not pass while it is paused or
//!   while the user types in its chat, and a whole interval is left once
//!   they stop.
//!
//! No window, no clock: the windows are `host/ui/monitor_windows.rs`'s.

use crate::ipc::MAX_SLOTS;
use crate::monitor::Spec;
use crate::panels::Place;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The height of a monitor's header, of its chat part, and the least and
/// first width of its window, in the screen's points.
pub const HEADER: u32 = 30;
pub const CHAT: u32 = 190;
pub const MIN_WIDTH: u32 = 220;
/// The least height of the picture's room.
pub const MIN_PICTURE: u32 = 90;

/// The shortest and the longest turn of the cycling monitor, in seconds,
/// and the one it has at first.
pub const MIN_INTERVAL_S: u32 = 2;
pub const MAX_INTERVAL_S: u32 = 600;
pub const DEFAULT_INTERVAL_S: u32 = 5;
/// The turns the options offer, in seconds.
pub const INTERVALS_S: [u32; 5] = [3, 5, 10, 30, 60];

/// What is remembered of one monitor's window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    /// Where the window was last (kept while the monitor is closed).
    pub place: Option<Place>,
    /// It stays over the other windows.
    pub pinned: bool,
    /// Its chat part is shown.
    pub chat: bool,
}

impl Window {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The window as the host can use it: a place no window can have had
    /// is dropped.
    pub fn cleaned(mut self) -> Self {
        self.place = self.place.filter(place_valid);
        self
    }

    /// The window was moved or resized; false when it is where it was, or
    /// that is no place for a window.
    pub fn placed(&mut self, place: Place) -> bool {
        if !place_valid(&place) || self.place == Some(place) {
            return false;
        }
        self.place = Some(place);
        true
    }
}

/// A place a monitor's window can have had: smaller than a panel's least
/// window, so only the numbers are checked.
fn place_valid(place: &Place) -> bool {
    const FAR: i32 = 1 << 16;
    (80..=16_384).contains(&place.size[0])
        && (60..=16_384).contains(&place.size[1])
        && place.position.is_none_or(|at| at.iter().all(|c| (-FAR..=FAR).contains(c)))
}

/// The inside of a monitor's window the first time it opens, and after
/// its size or its chat part was chosen: the picture's box in the
/// screen's own pixels under the header, and the chat part under it.
/// `scale`: the screen's pixels for a point.
pub fn natural_size(spec: Spec, chat: bool, scale: f64) -> [u32; 2] {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let (width, height) = spec.size.dims();
    let points = |points: u32| (f64::from(points) * scale).round() as u32;
    let chat = if chat { points(CHAT) } else { 0 };
    [width.max(points(MIN_WIDTH)), points(HEADER) + height + chat]
}

/// The cycling monitor in the settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cycling {
    /// Its window is open.
    pub open: bool,
    /// How long each character is shown, in seconds.
    pub interval_s: u32,
    /// The turn does not pass while the user types in its chat.
    pub pause_typing: bool,
    /// The size and rate of its pictures.
    pub spec: Spec,
    pub window: Window,
}

impl Default for Cycling {
    fn default() -> Self {
        Self {
            open: false,
            interval_s: DEFAULT_INTERVAL_S,
            pause_typing: true,
            spec: Spec::default(),
            window: Window::default(),
        }
    }
}

impl Cycling {
    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.interval_s.clamp(MIN_INTERVAL_S, MAX_INTERVAL_S)))
    }
}

/// The monitors in the host's settings file (`session-host.json`,
/// `monitors`). Each character's own monitor is with its character
/// (`monitor`, `monitor_window`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The size and rate a monitor opens with.
    pub defaults: Spec,
    pub cycling: Cycling,
}

impl Config {
    /// The file's monitors as the host can use them: a turn within
    /// [`MIN_INTERVAL_S`] and [`MAX_INTERVAL_S`], a place a window can
    /// have had.
    pub fn cleaned(mut self) -> Self {
        self.cycling.interval_s = self.cycling.interval_s.clamp(MIN_INTERVAL_S, MAX_INTERVAL_S);
        self.cycling.window = self.cycling.window.cleaned();
        self
    }
}

/// What one viewer is asked when two windows show its character: the
/// larger size and the faster rate of the two.
pub fn cover(a: Option<Spec>, b: Option<Spec>) -> Option<Spec> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Spec {
            size: if b.size.pixels() > a.size.pixels() { b.size } else { a.size },
            rate: if b.rate.interval_ms() < a.rate.interval_ms() {
                b.rate
            } else {
                a.rate
            },
        }),
        (one, other) => one.or(other),
    }
}

/// A monitor's window follows the character that is not in front.
/// `open`: the characters whose monitor is open; `from`: the character
/// that was in front; `to`: the one that comes to the front. Some((to,
/// from)) when the monitor of `to` moves to `from`: `to` has one, another
/// character left the front, and that one has none.
pub fn follow(open: [bool; MAX_SLOTS], from: Option<usize>, to: usize) -> Option<(usize, usize)> {
    let from = from.filter(|from| *from != to && *from < MAX_SLOTS)?;
    (open.get(to).copied().unwrap_or(false) && !open[from]).then_some((to, from))
}

/// The cycling monitor's turn: which background character it shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    shown: Option<usize>,
    /// When the turn of the one shown started (or last was held).
    since: Option<Instant>,
    paused: bool,
}

impl Cycle {
    /// The character shown.
    pub fn shown(&self) -> Option<usize> {
        self.shown
    }

    /// The user paused it.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pauses the turns, or lets them go on: a whole turn from now.
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        self.paused = paused;
        self.since = Some(now);
    }

    /// The candidate after (or before) `slot` in slot order, round and
    /// round; the first (or last) when `slot` is none of them.
    fn neighbour(candidates: &[usize], slot: Option<usize>, forward: bool) -> Option<usize> {
        let next = match (slot, forward) {
            (Some(slot), true) => candidates.iter().copied().find(|c| *c > slot),
            (Some(slot), false) => candidates.iter().rev().copied().find(|c| *c < slot),
            (None, _) => None,
        };
        next.or(if forward {
            candidates.first().copied()
        } else {
            candidates.last().copied()
        })
    }

    /// Now and then: the character to show. `candidates`: the characters
    /// that can be shown, in slot order; `hold`: the user types in the
    /// cycling monitor's chat. The turn passes after `interval`, not
    /// while held or paused; a whole turn is left once the hold ends. A
    /// character that cannot be shown any more gives its turn to the
    /// next one at once.
    pub fn tick(&mut self, candidates: &[usize], interval: Duration, hold: bool, now: Instant) -> Option<usize> {
        let Some(shown) = self.shown.filter(|shown| candidates.contains(shown)) else {
            self.shown = Self::neighbour(candidates, self.shown, true);
            self.since = self.shown.map(|_| now);
            return self.shown;
        };
        let since = *self.since.get_or_insert(now);
        if hold || self.paused {
            self.since = Some(now);
        } else if now.saturating_duration_since(since) >= interval {
            self.shown = Self::neighbour(candidates, Some(shown), true);
            self.since = Some(now);
        }
        self.shown
    }

    /// The user asks for the next or the previous character: shown for a
    /// whole turn from now.
    pub fn step(&mut self, candidates: &[usize], forward: bool, now: Instant) -> Option<usize> {
        self.shown = Self::neighbour(candidates, self.shown, forward);
        self.since = self.shown.map(|_| now);
        self.shown
    }

    /// That character is shown, for a whole turn from now (the one that
    /// left the front, after "Switch").
    pub fn show(&mut self, slot: usize, now: Instant) {
        self.shown = Some(slot);
        self.since = Some(now);
    }

    /// How much of the turn is left, from 1 (just started) to 0; None
    /// when nobody is shown or the turn does not pass.
    pub fn left(&self, interval: Duration, hold: bool, now: Instant) -> Option<f32> {
        let since = self.since.filter(|_| self.shown.is_some() && !self.paused && !hold)?;
        let gone = now.saturating_duration_since(since).as_secs_f32();
        Some((1.0 - gone / interval.as_secs_f32().max(0.001)).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Rate, Size};

    const TURN: Duration = Duration::from_secs(5);

    fn after(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn a_window_s_place_is_checked_and_kept() {
        let mut window = Window::default();
        assert!(window.is_default());
        let place = Place {
            position: Some([40, 60]),
            size: [480, 300],
        };
        assert!(window.placed(place));
        assert!(!window.placed(place), "where it was: nothing to save");
        // no place for a window: not kept
        let none = Place {
            position: Some([0, 0]),
            size: [10, 300],
        };
        assert!(!window.placed(none));
        assert_eq!(window.place, Some(place));
        // nor is one farther than any desktop reaches
        let far = Place {
            position: Some([1 << 20, 0]),
            size: [480, 300],
        };
        assert!(!window.placed(far));
        // a file's place that is none is dropped, the rest kept
        let read = Window {
            place: Some(none),
            pinned: true,
            chat: true,
        }
        .cleaned();
        assert_eq!(
            read,
            Window {
                place: None,
                pinned: true,
                chat: true
            }
        );
        assert!(!read.is_default());
    }

    #[test]
    fn a_window_opens_as_large_as_its_picture_and_its_chat() {
        let small = Spec {
            size: Size::Small,
            rate: Rate::One,
        };
        assert_eq!(natural_size(small, false, 1.0), [320, HEADER + 180]);
        assert_eq!(natural_size(small, true, 1.0), [320, HEADER + 180 + CHAT]);
        // the picture is in the screen's own pixels; the rest scales
        assert_eq!(natural_size(small, false, 2.0), [2 * MIN_WIDTH, 2 * HEADER + 180]);
        let large = Spec {
            size: Size::Large,
            rate: Rate::One,
        };
        assert_eq!(natural_size(large, true, 1.5), [640, 45 + 360 + 285]);
        // a scale no screen has is 1
        assert_eq!(natural_size(small, false, f64::NAN), natural_size(small, false, 1.0));
        assert_eq!(natural_size(small, false, 0.0), natural_size(small, false, 1.0));
        // every first size is a place a window can have
        for size in Size::ALL {
            for chat in [false, true] {
                let size = natural_size(Spec { size, rate: Rate::One }, chat, 1.0);
                assert!(place_valid(&Place { position: None, size }));
            }
        }
    }

    #[test]
    fn the_monitors_settings_are_checked_when_read() {
        let config = Config::default();
        assert!(!config.cycling.open && config.cycling.pause_typing);
        assert_eq!(config.cycling.interval_s, DEFAULT_INTERVAL_S);
        assert_eq!(config.cycling.interval(), Duration::from_secs(5));
        assert_eq!(config.defaults, Spec::default());
        assert_eq!(config.cleaned(), config);
        assert!(INTERVALS_S.contains(&DEFAULT_INTERVAL_S));
        assert!(INTERVALS_S.iter().all(|s| (MIN_INTERVAL_S..=MAX_INTERVAL_S).contains(s)));
        // a turn no setting offers is brought within the bounds
        let read: Config =
            serde_json::from_str(r#"{"cycling":{"open":true,"interval_s":0,"window":{"place":{"size":[1,1]}}}}"#).expect("reads");
        let read = read.cleaned();
        assert!(read.cycling.open && read.cycling.pause_typing);
        assert_eq!(read.cycling.interval_s, MIN_INTERVAL_S);
        assert_eq!(read.cycling.window.place, None);
        let long: Config = serde_json::from_str(r#"{"cycling":{"interval_s":4000000000}}"#).expect("reads");
        assert_eq!(long.cleaned().cycling.interval_s, MAX_INTERVAL_S);
        // nothing of it in the file: the defaults
        let empty: Config = serde_json::from_str("{}").expect("reads");
        assert_eq!(empty, Config::default());
        // and it reads back what it writes
        let written = serde_json::to_string(&read).expect("writes");
        assert_eq!(serde_json::from_str::<Config>(&written).expect("reads"), read);
    }

    #[test]
    fn two_windows_on_one_character_ask_for_the_larger_and_the_faster() {
        let spec = |size, rate| Some(Spec { size, rate });
        assert_eq!(cover(None, None), None);
        assert_eq!(cover(spec(Size::Small, Rate::Two), None), spec(Size::Small, Rate::Two));
        assert_eq!(cover(None, spec(Size::Large, Rate::Half)), spec(Size::Large, Rate::Half));
        assert_eq!(
            cover(spec(Size::Small, Rate::Five), spec(Size::Large, Rate::Half)),
            spec(Size::Large, Rate::Five)
        );
        assert_eq!(
            cover(spec(Size::Large, Rate::Half), spec(Size::Small, Rate::Five)),
            spec(Size::Large, Rate::Five)
        );
        assert_eq!(
            cover(spec(Size::Medium, Rate::One), spec(Size::Medium, Rate::One)),
            spec(Size::Medium, Rate::One)
        );
    }

    #[test]
    fn a_monitor_follows_the_character_that_is_not_in_front() {
        let open = |slots: &[usize]| std::array::from_fn(|slot| slots.contains(&slot));
        // 1 is watched and comes to the front; 0 leaves it and has no
        // monitor: the monitor is 0's
        assert_eq!(follow(open(&[1]), Some(0), 1), Some((1, 0)));
        // nobody was in front: nothing to show in its place
        assert_eq!(follow(open(&[1]), None, 1), None);
        // the one that comes to the front has no monitor: nothing moves
        assert_eq!(follow(open(&[1]), Some(0), 2), None);
        // the one that left has its own already
        assert_eq!(follow(open(&[0, 1]), Some(0), 1), None);
        // brought forward again while in front
        assert_eq!(follow(open(&[1]), Some(1), 1), None);
        // every background character watched: a switch keeps it so
        let mut watched = open(&[1, 2, 3, 4]);
        let mut front = 0;
        for to in [3, 1, 4, 0, 2] {
            let (from_slot, to_slot) = follow(watched, Some(front), to).expect("it moves");
            watched[from_slot] = false;
            watched[to_slot] = true;
            front = to;
            assert!(!watched[front]);
            assert_eq!(watched.iter().filter(|w| **w).count(), 4);
        }
        // slots there are not
        assert_eq!(follow(open(&[1]), Some(MAX_SLOTS), 1), None);
        assert_eq!(follow(open(&[1]), Some(0), MAX_SLOTS), None);
    }

    #[test]
    fn the_cycling_monitor_shows_each_background_character_in_turn() {
        let start = Instant::now();
        let mut cycle = Cycle::default();
        assert_eq!(cycle.shown(), None);
        // nobody to show
        assert_eq!(cycle.tick(&[], TURN, false, start), None);
        // the first one, then each for a turn, round and round
        let candidates = [1, 2, 4];
        assert_eq!(cycle.tick(&candidates, TURN, false, start), Some(1));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 4)), Some(1));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 5)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 9)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 10)), Some(4));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 15)), Some(1));
        // a look that comes late passes one turn, not several
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 60)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 61)), Some(2));
    }

    #[test]
    fn a_character_that_cannot_be_shown_gives_its_turn_at_once() {
        let start = Instant::now();
        let mut cycle = Cycle::default();
        assert_eq!(cycle.tick(&[1, 2, 4], TURN, false, start), Some(1));
        assert_eq!(cycle.tick(&[1, 2, 4], TURN, false, after(start, 5)), Some(2));
        // 2 came to the front (or left the world): the next one, for a
        // whole turn
        assert_eq!(cycle.tick(&[1, 4], TURN, false, after(start, 6)), Some(4));
        assert_eq!(cycle.tick(&[1, 4], TURN, false, after(start, 10)), Some(4));
        assert_eq!(cycle.tick(&[1, 4], TURN, false, after(start, 11)), Some(1));
        // the last of the row left: round to the first
        let mut cycle = Cycle::default();
        cycle.show(4, start);
        assert_eq!(cycle.tick(&[0, 1], TURN, false, start), Some(0));
        // nobody left, then someone again
        assert_eq!(cycle.tick(&[], TURN, false, after(start, 1)), None);
        assert_eq!(cycle.tick(&[3], TURN, false, after(start, 2)), Some(3));
        // one alone keeps being shown
        assert_eq!(cycle.tick(&[3], TURN, false, after(start, 30)), Some(3));
    }

    #[test]
    fn the_turn_does_not_pass_while_the_user_types() {
        let start = Instant::now();
        let mut cycle = Cycle::default();
        let candidates = [1, 2];
        assert_eq!(cycle.tick(&candidates, TURN, false, start), Some(1));
        // typing starts at 4 s and goes on long past the turn
        for second in 4..=40 {
            assert_eq!(cycle.tick(&candidates, TURN, true, after(start, second)), Some(1), "{second}");
        }
        // sent at 40 s: a whole turn is left, the character does not
        // change under the line just said
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 41)), Some(1));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 44)), Some(1));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 45)), Some(2));
        // held from the very look that would have passed the turn
        assert_eq!(cycle.tick(&candidates, TURN, true, after(start, 50)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 54)), Some(2));
        // the character typed for cannot be shown any more: the turn
        // passes (its draft stays its own: `chat::Chat`)
        assert_eq!(cycle.tick(&[1], TURN, true, after(start, 55)), Some(1));
    }

    #[test]
    fn pause_previous_and_next() {
        let start = Instant::now();
        let mut cycle = Cycle::default();
        let candidates = [0, 2, 3];
        assert_eq!(cycle.tick(&candidates, TURN, false, start), Some(0));
        // next and previous, round and round, each for a whole turn
        assert_eq!(cycle.step(&candidates, true, after(start, 4)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 8)), Some(2));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 9)), Some(3));
        assert_eq!(cycle.step(&candidates, true, after(start, 9)), Some(0));
        assert_eq!(cycle.step(&candidates, false, after(start, 9)), Some(3));
        assert_eq!(cycle.step(&candidates, false, after(start, 9)), Some(2));
        assert_eq!(cycle.step(&[], true, after(start, 9)), None);
        assert_eq!(cycle.step(&candidates, false, after(start, 9)), Some(3), "from nobody: the last");
        // paused: it stays, however long
        cycle.set_paused(true, after(start, 10));
        assert!(cycle.paused());
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 100)), Some(3));
        assert_eq!(cycle.left(TURN, false, after(start, 100)), None);
        // a step is still the user's to take
        assert_eq!(cycle.step(&candidates, true, after(start, 100)), Some(0));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 200)), Some(0));
        // on again: a whole turn from then
        cycle.set_paused(false, after(start, 200));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 204)), Some(0));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 205)), Some(2));
        // after "Switch": the one that left the front
        cycle.show(3, after(start, 206));
        assert_eq!(cycle.tick(&candidates, TURN, false, after(start, 210)), Some(3));
        assert_eq!(cycle.left(TURN, false, after(start, 206)), Some(1.0));
        assert_eq!(cycle.left(TURN, true, after(start, 206)), None);
        let half = cycle.left(Duration::from_secs(8), false, after(start, 210)).expect("a turn");
        assert!((half - 0.5).abs() < 0.01);
    }
}
