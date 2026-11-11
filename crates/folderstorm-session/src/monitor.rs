//! Monitors (session-host plan, section 12, checkpoint 5): a small picture
//! of what a background character's own viewer shows, in the host's
//! window, a few times a second at most.
//!
//! - **The user's choice** per character is a [`Spec`]: one of three sizes
//!   and four rates. At most [`MAX_MONITORS`] run at once.
//! - **An aggregate budget.** All the monitors together may ask the card
//!   for [`BUDGET`] pixels a second; past it every monitor's rate is
//!   lowered by the same factor ([`grant`]). What a viewer is asked is an
//!   [`Order`]: the size and the time between two pictures.
//! - **In the viewer** the 3D stays off between two pictures. A
//!   [`Schedule`] says when the next one is asked, never two at once, and
//!   lowers the rate by itself when the card is slow to answer: a picture
//!   that took long or never came back doubles the time to the next one, a
//!   card the viewer already found busy (Aurora's busy pause) gets one
//!   picture every [`BUSY_INTERVAL`] at most, and a card that holds a frame
//!   or is lost gets none. A monitor never pushes a busy card.
//! - **The picture** has the proportions of the character's own window,
//!   fitted in the size's box ([`fit`]), and travels as plain RGB after a
//!   [`FrameHeader`] that says its exact size; anything else is refused
//!   before a byte of it is read ([`FrameHeader::payload`]). Only the
//!   latest picture is ever kept ([`Latest`]): one that was not sent or
//!   shown in time is replaced, never queued.
//!
//! Nothing here draws, reads a clock or opens a pipe.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How many monitors run at once: one per character that is not the
/// active one (decision 3 of the plan).
pub const MAX_MONITORS: usize = 4;

/// Pixels a second all the monitors together may ask of the card: four
/// large monitors at one picture a second (921 600), or one large monitor
/// at five (1 152 000), pass; two large ones at five do not.
pub const BUDGET: u64 = 1_200_000;

/// The shortest and the longest time between two pictures an order may
/// carry, in milliseconds.
pub const MIN_INTERVAL_MS: u32 = 200;
pub const MAX_INTERVAL_MS: u32 = 60_000;

/// The smallest side of a picture.
pub const MIN_SIDE: u32 = 16;
/// The largest picture: the large size's box.
pub const MAX_WIDTH: u32 = 640;
pub const MAX_HEIGHT: u32 = 360;
/// The most bytes a picture takes on the wire (RGB, 691 200).
pub const MAX_FRAME_BYTES: usize = (MAX_WIDTH * MAX_HEIGHT * 3) as usize;

/// A monitor's size: the box its picture fits in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    /// 320 × 180.
    Small,
    /// 480 × 270.
    #[default]
    Medium,
    /// 640 × 360.
    Large,
}

impl Size {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    /// The box, width and height in pixels.
    pub fn dims(self) -> (u32, u32) {
        match self {
            Self::Small => (320, 180),
            Self::Medium => (480, 270),
            Self::Large => (MAX_WIDTH, MAX_HEIGHT),
        }
    }

    pub fn pixels(self) -> u64 {
        let (w, h) = self.dims();
        u64::from(w) * u64::from(h)
    }
}

/// A monitor's rate, as the user chooses it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rate {
    /// One picture every two seconds.
    Half,
    /// One a second.
    #[default]
    One,
    Two,
    Five,
}

impl Rate {
    pub const ALL: [Self; 4] = [Self::Half, Self::One, Self::Two, Self::Five];

    /// The time between two pictures, in milliseconds.
    pub fn interval_ms(self) -> u32 {
        match self {
            Self::Half => 2000,
            Self::One => 1000,
            Self::Two => 500,
            Self::Five => MIN_INTERVAL_MS,
        }
    }
}

/// The user's choice for one character's monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub size: Size,
    pub rate: Rate,
}

/// What a viewer is asked while its monitor is watched: pictures that fit
/// `size`, one every `interval_ms` at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub size: Size,
    pub interval_ms: u32,
}

impl Order {
    /// The order with its interval inside what a viewer accepts: never
    /// faster than the fastest rate, whatever came down the pipe.
    pub fn cleaned(mut self) -> Self {
        self.interval_ms = self.interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
        self
    }

    pub fn interval(self) -> Duration {
        Duration::from_millis(u64::from(self.cleaned().interval_ms))
    }
}

/// Pixels a second a set of orders asks of the card (rounded up).
pub fn load(orders: &[Order]) -> u64 {
    orders
        .iter()
        .map(|o| (o.size.pixels() * 1000).div_ceil(u64::from(o.cleaned().interval_ms)))
        .sum()
}

/// What each monitor is granted of what the user chose: as chosen while
/// all of them fit [`BUDGET`], otherwise every interval stretched by the
/// same factor so that they do. Only the first [`MAX_MONITORS`] are
/// granted anything.
pub fn grant(wanted: &[Spec]) -> Vec<Order> {
    let wanted = &wanted[..wanted.len().min(MAX_MONITORS)];
    let chosen: Vec<Order> = wanted
        .iter()
        .map(|s| Order {
            size: s.size,
            interval_ms: s.rate.interval_ms(),
        })
        .collect();
    // (exact, in pixels per 1000 seconds: no rounding before the division)
    let total: u64 = wanted
        .iter()
        .map(|s| s.size.pixels() * 1_000_000 / u64::from(s.rate.interval_ms()))
        .sum();
    let budget = BUDGET * 1000;
    if total <= budget {
        return chosen;
    }
    chosen
        .into_iter()
        .map(|o| {
            let stretched = (u64::from(o.interval_ms) * total).div_ceil(budget);
            Order {
                size: o.size,
                interval_ms: u32::try_from(stretched).unwrap_or(MAX_INTERVAL_MS),
            }
            .cleaned()
        })
        .collect()
}

/// The size of a picture for a window of `window` pixels: the largest one
/// with the window's proportions that fits the box of `size`, its sides
/// even and at least [`MIN_SIDE`] (a window far wider or taller than any
/// screen is then cut, not squeezed to nothing). None for a window without
/// a size.
pub fn fit(size: Size, window: (u32, u32)) -> Option<(u32, u32)> {
    let (ww, wh) = (u64::from(window.0), u64::from(window.1));
    if ww == 0 || wh == 0 {
        return None;
    }
    let (bw, bh) = size.dims();
    let (bw64, bh64) = (u64::from(bw), u64::from(bh));
    // wider than the box: the width is the box's, the height follows
    let (w, h) = if ww * bh64 >= wh * bw64 {
        (bw64, bw64 * wh / ww)
    } else {
        (bh64 * ww / wh, bh64)
    };
    let even = |side: u64, most: u32| ((side as u32) & !1).clamp(MIN_SIDE, most);
    Some((even(w, bw), even(h, bh)))
}

/// What comes before a picture on the wire. The picture is the `bytes`
/// that follow it: `width` × `height` pixels of three bytes (red, green,
/// blue), rows from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHeader {
    /// The login the picture was taken in ([`crate::ipc::Status::generation`]).
    pub generation: u32,
    /// Grows with every picture a viewer takes.
    pub seq: u64,
    pub width: u16,
    pub height: u16,
    pub bytes: u32,
}

impl FrameHeader {
    /// The header of a picture of that size; None when no monitor shows
    /// such a size.
    pub fn new(generation: u32, seq: u64, width: u32, height: u32) -> Option<Self> {
        let header = Self {
            generation,
            seq,
            width: u16::try_from(width).ok()?,
            height: u16::try_from(height).ok()?,
            bytes: width.checked_mul(height)?.checked_mul(3)?,
        };
        header.payload().map(|_| header)
    }

    /// How many bytes follow, when the header is one a monitor can show:
    /// both sides within [`MIN_SIDE`] and the largest box, and `bytes`
    /// exactly what such a picture takes. None otherwise: nothing of what
    /// follows is read, and the stream is out of step.
    pub fn payload(&self) -> Option<usize> {
        let (w, h) = (u32::from(self.width), u32::from(self.height));
        if !(MIN_SIDE..=MAX_WIDTH).contains(&w) || !(MIN_SIDE..=MAX_HEIGHT).contains(&h) {
            return None;
        }
        // (the sides are in the box: no overflow, MAX_FRAME_BYTES at most)
        let bytes = (w * h * 3) as usize;
        (self.bytes as usize == bytes).then_some(bytes)
    }

    /// Whether the picture fits the box of `size`: what a monitor of that
    /// size shows. A picture taken for a larger size is dropped.
    pub fn fits(&self, size: Size) -> bool {
        let (bw, bh) = size.dims();
        self.payload().is_some() && u32::from(self.width) <= bw && u32::from(self.height) <= bh
    }
}

/// The picture as it travels: the red, green and blue of `rgba`'s pixels
/// (a picture of the world is opaque). None when `rgba` is not `width` ×
/// `height` pixels of four bytes.
pub fn rgb_from_rgba(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if rgba.len() != pixels.checked_mul(4)? {
        return None;
    }
    let mut rgb = Vec::with_capacity(pixels * 3);
    for pixel in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    Some(rgb)
}

/// A place for one thing: what is put in replaces what was not taken yet.
/// A picture nobody had the time to send or to show is never queued behind
/// a newer one.
#[derive(Debug)]
pub struct Latest<T> {
    held: Option<T>,
    replaced: u64,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self { held: None, replaced: 0 }
    }
}

impl<T> Latest<T> {
    /// Puts `item` in; true when it took the place of one not taken.
    pub fn put(&mut self, item: T) -> bool {
        let replaced = self.held.replace(item).is_some();
        self.replaced += u64::from(replaced);
        replaced
    }

    pub fn take(&mut self) -> Option<T> {
        self.held.take()
    }

    /// Empties the place (the monitor closed: nothing of it is kept).
    pub fn clear(&mut self) {
        self.held = None;
    }

    /// How many were replaced before anything took them.
    pub fn replaced(&self) -> u64 {
        self.replaced
    }
}

/// What a viewer says of its monitor, in its status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// No monitor is asked of it.
    #[default]
    Off,
    /// Pictures are taken at the rate asked.
    Live,
    /// Pictures are taken at a lower rate: the card is slow to answer.
    Slowed,
    /// No picture: the card holds a frame, is lost, or is out of memory.
    CardBusy,
    /// No picture: the window is minimised (Aurora gave its textures back).
    Minimised,
    /// No picture: the character is not in the world.
    NotInWorld,
    /// No picture for now: the viewer draws something of its own (a photo,
    /// a timed run).
    Occupied,
}

/// What a viewer knows when it decides on a picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Conditions {
    /// The character is in the world.
    pub in_world: bool,
    pub minimised: bool,
    /// The card holds a frame, is lost, or is out of memory: nothing more
    /// is asked of it.
    pub card_gone: bool,
    /// Aurora's busy pause is on: the card is busy with another program.
    pub card_busy: bool,
    /// The viewer draws a photo or runs a timed capture.
    pub occupied: bool,
}

/// A picture that took at least this long to come back is slow: the same
/// measure as Aurora's busy pause (`busy_pause.rs`, `SLOW_MS`).
pub const SLOW_PICTURE: Duration = Duration::from_millis(150);
/// A picture not back after this long is given up (the driver's own limit
/// for a frame is 2 s).
pub const PICTURE_TIMEOUT: Duration = Duration::from_millis(1500);
/// While Aurora's busy pause is on: one picture this often at most.
pub const BUSY_INTERVAL: Duration = Duration::from_secs(10);
/// How many times the time between two pictures may double.
pub const MAX_LEVEL: u8 = 5;
/// Quick pictures in a row before the rate comes back up one step.
pub const QUICK_STREAK: u8 = 4;

/// When a viewer takes its monitor's pictures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Schedule {
    /// The picture asked and not answered yet, since when.
    asked: Option<Instant>,
    /// When the last picture was asked.
    last: Option<Instant>,
    /// The time between two pictures is doubled this many times.
    level: u8,
    /// Quick pictures in a row.
    quick: u8,
}

impl Schedule {
    /// Why no picture is taken now, if so. A card in trouble comes first:
    /// it is what the user must know.
    fn held(c: Conditions) -> Option<State> {
        if c.card_gone {
            Some(State::CardBusy)
        } else if !c.in_world {
            Some(State::NotInWorld)
        } else if c.minimised {
            Some(State::Minimised)
        } else if c.occupied {
            Some(State::Occupied)
        } else {
            None
        }
    }

    /// The time between two pictures now: the order's, doubled once per
    /// level, and never under [`BUSY_INTERVAL`] while the card is busy.
    pub fn interval(&self, order: Order, c: Conditions) -> Duration {
        let lowered = order.interval().saturating_mul(1 << self.level.min(MAX_LEVEL));
        let lowered = lowered.min(Duration::from_millis(u64::from(MAX_INTERVAL_MS)));
        if c.card_busy { lowered.max(BUSY_INTERVAL) } else { lowered }
    }

    /// What the host is told.
    pub fn state(&self, order: Option<Order>, c: Conditions) -> State {
        match order {
            None => State::Off,
            Some(_) => Self::held(c).unwrap_or(if c.card_busy || self.level > 0 {
                State::Slowed
            } else {
                State::Live
            }),
        }
    }

    /// Whether a picture is asked now. True at most once per interval,
    /// never while one is still awaited, never without an order, never
    /// when something holds the monitor. The caller then asks its renderer
    /// and reports what became of it ([`Self::answered`], [`Self::failed`],
    /// [`Self::timed_out`]).
    pub fn poll(&mut self, order: Option<Order>, c: Conditions, now: Instant) -> bool {
        let Some(order) = order else {
            // the monitor closed: the next one starts at its own rate
            self.level = 0;
            self.quick = 0;
            return false;
        };
        if self.asked.is_some() || Self::held(c).is_some() {
            return false;
        }
        let due = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= self.interval(order, c));
        if due {
            self.asked = Some(now);
            self.last = Some(now);
        }
        due
    }

    /// A picture is awaited.
    pub fn awaiting(&self) -> bool {
        self.asked.is_some()
    }

    /// The picture awaited has been so for longer than
    /// [`PICTURE_TIMEOUT`]: the caller gives it up ([`Self::timed_out`]).
    pub fn overdue(&self, now: Instant) -> bool {
        self.asked
            .is_some_and(|asked| now.saturating_duration_since(asked) >= PICTURE_TIMEOUT)
    }

    /// The picture came back. A slow one lowers the rate a step; enough
    /// quick ones in a row bring it back up a step.
    pub fn answered(&mut self, now: Instant) {
        let Some(asked) = self.asked.take() else {
            return;
        };
        if now.saturating_duration_since(asked) >= SLOW_PICTURE {
            self.slower(1);
        } else {
            self.quick += 1;
            if self.quick >= QUICK_STREAK {
                self.quick = 0;
                self.level = self.level.saturating_sub(1);
            }
        }
    }

    /// The picture could not be drawn (the device was lost under it).
    pub fn failed(&mut self) {
        if self.asked.take().is_some() {
            self.slower(1);
        }
    }

    /// The picture never came back in time and was given up: the card is
    /// far behind, the rate drops two steps.
    pub fn timed_out(&mut self) {
        if self.asked.take().is_some() {
            self.slower(2);
        }
    }

    /// The renderer was made again (a recovery): what was asked of the old
    /// one never comes back.
    pub fn forget(&mut self) {
        self.asked = None;
    }

    fn slower(&mut self, steps: u8) {
        self.quick = 0;
        self.level = self.level.saturating_add(steps).min(MAX_LEVEL);
    }

    /// How many times the rate is halved now.
    pub fn level(&self) -> u8 {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(size: Size, rate: Rate) -> Spec {
        Spec { size, rate }
    }

    const FINE: Conditions = Conditions {
        in_world: true,
        minimised: false,
        card_gone: false,
        card_busy: false,
        occupied: false,
    };

    fn order() -> Order {
        Order {
            size: Size::Large,
            interval_ms: 1000,
        }
    }

    #[test]
    fn the_sizes_and_rates_are_the_plan_s() {
        assert_eq!(Size::ALL.map(Size::dims), [(320, 180), (480, 270), (640, 360)]);
        assert_eq!(Rate::ALL.map(Rate::interval_ms), [2000, 1000, 500, 200]);
        assert_eq!(Spec::default().rate, Rate::One, "one picture a second by default");
        assert_eq!(MAX_FRAME_BYTES, 691_200);
    }

    #[test]
    fn monitors_within_the_budget_get_what_was_chosen() {
        // the acceptance's four monitors at one picture a second, at any size
        for size in Size::ALL {
            let wanted = [spec(size, Rate::One); 4];
            let granted = grant(&wanted);
            assert_eq!(granted.len(), 4);
            assert!(granted.iter().all(|o| o.size == size && o.interval_ms == 1000), "{size:?}");
            assert!(load(&granted) <= BUDGET);
        }
        // one large monitor at the fastest rate
        assert_eq!(grant(&[spec(Size::Large, Rate::Five)])[0].interval_ms, 200);
        assert!(grant(&[]).is_empty());
    }

    #[test]
    fn past_the_budget_every_rate_is_lowered_by_the_same_factor() {
        let wanted = [spec(Size::Large, Rate::Five), spec(Size::Large, Rate::Five)];
        let granted = grant(&wanted);
        assert_eq!(granted[0], granted[1]);
        assert!(granted[0].interval_ms > 200, "{granted:?}");
        assert!(load(&granted) <= BUDGET, "{}", load(&granted));
        // mixed: the slow one is stretched as much as the fast one
        let wanted = [
            spec(Size::Large, Rate::Five),
            spec(Size::Large, Rate::Five),
            spec(Size::Small, Rate::Half),
        ];
        let granted = grant(&wanted);
        assert_eq!(granted[2].size, Size::Small);
        let factor = |i: usize| f64::from(granted[i].interval_ms) / f64::from(wanted[i].rate.interval_ms());
        assert!((factor(0) - factor(2)).abs() < 0.02, "{granted:?}");
        assert!(load(&granted) <= BUDGET);
        // the worst the window can ask: every combination stays in the budget
        for size in Size::ALL {
            for rate in Rate::ALL {
                let granted = grant(&[spec(size, rate); MAX_MONITORS]);
                assert!(load(&granted) <= BUDGET, "{size:?} {rate:?}: {}", load(&granted));
                assert!(
                    granted.iter().all(|o| o.interval_ms >= rate.interval_ms()),
                    "never faster than chosen"
                );
            }
        }
    }

    #[test]
    fn no_more_monitors_than_other_characters() {
        let granted = grant(&[spec(Size::Small, Rate::One); 7]);
        assert_eq!(granted.len(), MAX_MONITORS);
    }

    #[test]
    fn an_order_never_asks_faster_than_the_fastest_rate() {
        let hurried = Order {
            size: Size::Large,
            interval_ms: 0,
        };
        assert_eq!(hurried.cleaned().interval_ms, MIN_INTERVAL_MS);
        assert_eq!(hurried.interval(), Duration::from_millis(200));
        let never = Order {
            size: Size::Small,
            interval_ms: u32::MAX,
        };
        assert_eq!(never.cleaned().interval_ms, MAX_INTERVAL_MS);
        // and its load is counted as cleaned: no division by zero
        assert_eq!(load(&[hurried]), 640 * 360 * 5);
    }

    #[test]
    fn a_picture_has_the_window_s_proportions_inside_the_box() {
        // 16:9 windows fill the box
        assert_eq!(fit(Size::Large, (1920, 1080)), Some((640, 360)));
        assert_eq!(fit(Size::Small, (2560, 1440)), Some((320, 180)));
        // 4:3: as tall as the box, narrower
        assert_eq!(fit(Size::Large, (1600, 1200)), Some((480, 360)));
        // wider than 16:9: as wide as the box, lower
        assert_eq!(fit(Size::Large, (3440, 1440)), Some((640, 266)));
        // portrait
        assert_eq!(fit(Size::Medium, (1080, 1920)), Some((150, 270)));
        // sides are even, never under the smallest side, never over the box
        assert_eq!(fit(Size::Small, (10_000, 10)), Some((320, MIN_SIDE)));
        assert_eq!(fit(Size::Small, (10, 10_000)), Some((MIN_SIDE, 180)));
        assert_eq!(fit(Size::Large, (u32::MAX, u32::MAX)), Some((360, 360)));
        for window in [(1366, 768), (1280, 1024), (801, 601), (333, 777)] {
            for size in Size::ALL {
                let (w, h) = fit(size, window).expect("a size");
                let (bw, bh) = size.dims();
                assert!(w % 2 == 0 && h % 2 == 0 && w <= bw && h <= bh, "{window:?} {size:?}: {w} x {h}");
                assert!(FrameHeader::new(0, 0, w, h).is_some_and(|f| f.fits(size)));
            }
        }
        assert_eq!(fit(Size::Large, (0, 600)), None);
        assert_eq!(fit(Size::Large, (800, 0)), None);
    }

    #[test]
    fn a_header_is_refused_unless_it_is_exactly_a_monitor_s_picture() {
        let good = FrameHeader::new(3, 9, 640, 360).expect("the largest picture");
        assert_eq!(good.payload(), Some(MAX_FRAME_BYTES));
        assert_eq!(FrameHeader::new(0, 0, 16, 16).and_then(|f| f.payload()), Some(768));
        // too large, too small, or a size that does not multiply out
        assert_eq!(FrameHeader::new(0, 0, 642, 360), None);
        assert_eq!(FrameHeader::new(0, 0, 640, 362), None);
        assert_eq!(FrameHeader::new(0, 0, 8, 100), None);
        assert_eq!(FrameHeader::new(0, 0, 100, 0), None);
        assert_eq!(FrameHeader::new(0, 0, u32::MAX, u32::MAX), None);
        for wrong in [0, 1, good.bytes - 1, good.bytes + 1, u32::MAX] {
            assert_eq!(FrameHeader { bytes: wrong, ..good }.payload(), None, "{wrong}");
        }
        assert_eq!(FrameHeader { width: 641, ..good }.payload(), None);
        assert_eq!(
            FrameHeader {
                width: u16::MAX,
                height: u16::MAX,
                bytes: u32::MAX,
                ..good
            }
            .payload(),
            None
        );
        // a picture taken for a larger monitor does not fit a smaller one
        assert!(good.fits(Size::Large));
        assert!(!good.fits(Size::Medium));
        let small = FrameHeader::new(0, 0, 320, 180).expect("small");
        assert!(Size::ALL.iter().all(|size| small.fits(*size)));
    }

    #[test]
    fn the_picture_travels_as_rgb_of_the_exact_size() {
        let rgba = [1, 2, 3, 255, 4, 5, 6, 0, 7, 8, 9, 128, 10, 11, 12, 255];
        assert_eq!(rgb_from_rgba(&rgba, 2, 2), Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
        assert_eq!(rgb_from_rgba(&rgba, 4, 1).map(|rgb| rgb.len()), Some(12));
        assert_eq!(rgb_from_rgba(&rgba, 2, 3), None);
        assert_eq!(rgb_from_rgba(&rgba[..15], 2, 2), None);
        assert_eq!(rgb_from_rgba(&rgba, u32::MAX, u32::MAX), None);
        let full = vec![7u8; 640 * 360 * 4];
        assert_eq!(rgb_from_rgba(&full, 640, 360).map(|rgb| rgb.len()), Some(MAX_FRAME_BYTES));
    }

    #[test]
    fn only_the_latest_picture_is_kept() {
        let mut latest = Latest::default();
        assert_eq!(latest.take(), None::<u32>);
        assert!(!latest.put(1));
        assert!(latest.put(2), "the first one was never taken: replaced");
        assert!(latest.put(3));
        assert_eq!(latest.take(), Some(3));
        assert_eq!(latest.take(), None);
        assert_eq!(latest.replaced(), 2);
        assert!(!latest.put(4));
        latest.clear();
        assert_eq!(latest.take(), None);
    }

    #[test]
    fn pictures_are_asked_one_at_a_time_at_the_order_s_rate() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        assert!(!s.poll(None, FINE, t0), "no monitor: nothing is asked");
        assert_eq!(s.state(None, FINE), State::Off);
        assert!(s.poll(Some(order()), FINE, t0), "the first picture at once");
        assert!(s.awaiting());
        // never a second one while the first is awaited, however late
        assert!(!s.poll(Some(order()), FINE, ms(5000)));
        s.answered(ms(20));
        assert!(!s.poll(Some(order()), FINE, ms(500)));
        assert!(!s.poll(Some(order()), FINE, ms(999)));
        assert!(s.poll(Some(order()), FINE, ms(1000)));
        s.answered(ms(1010));
        assert_eq!(s.state(Some(order()), FINE), State::Live);
        assert_eq!(s.level(), 0);
    }

    #[test]
    fn a_slow_card_lowers_the_rate_and_quick_pictures_bring_it_back() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        assert!(s.poll(Some(order()), FINE, t0));
        s.answered(ms(400));
        assert_eq!(s.level(), 1);
        assert_eq!(s.interval(order(), FINE), Duration::from_secs(2));
        assert_eq!(s.state(Some(order()), FINE), State::Slowed);
        assert!(!s.poll(Some(order()), FINE, ms(1500)), "not at the order's rate any more");
        assert!(s.poll(Some(order()), FINE, ms(2000)));
        // given up: two steps at once
        assert!(!s.overdue(ms(2000 + 1499)));
        assert!(s.overdue(ms(2000 + 1500)));
        s.timed_out();
        assert!(!s.awaiting());
        assert_eq!(s.level(), 3);
        assert_eq!(s.interval(order(), FINE), Duration::from_secs(8));
        // a device lost under a picture: one step
        assert!(s.poll(Some(order()), FINE, ms(10_000)));
        s.failed();
        assert_eq!(s.level(), 4);
        // never past the last level, never past the longest interval
        for _ in 0..20 {
            s.asked = Some(t0);
            s.timed_out();
        }
        assert_eq!(s.level(), MAX_LEVEL);
        assert_eq!(s.interval(order(), FINE), Duration::from_secs(32));
        let slow = Order {
            size: Size::Small,
            interval_ms: 30_000,
        };
        assert_eq!(s.interval(slow, FINE), Duration::from_secs(60));
        // quick pictures: one step back per streak
        let mut at = 100_000;
        for step in (0..MAX_LEVEL).rev() {
            for _ in 0..QUICK_STREAK {
                s.asked = Some(ms(at));
                s.answered(ms(at + 10));
                at += 100;
            }
            assert_eq!(s.level(), step);
        }
        assert_eq!(s.state(Some(order()), FINE), State::Live);
        // a slow one in the middle of a streak starts the count again
        for _ in 0..QUICK_STREAK - 1 {
            s.asked = Some(ms(at));
            s.answered(ms(at + 10));
        }
        s.asked = Some(ms(at));
        s.answered(ms(at + 150));
        assert_eq!(s.level(), 1);
        s.asked = Some(ms(at));
        s.answered(ms(at + 10));
        assert_eq!(s.level(), 1, "one quick picture is not a streak");
    }

    #[test]
    fn a_busy_card_gets_few_pictures_and_a_card_in_trouble_none() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let busy = Conditions { card_busy: true, ..FINE };
        assert!(s.poll(Some(order()), busy, t0));
        s.answered(ms(30));
        assert_eq!(s.state(Some(order()), busy), State::Slowed);
        assert_eq!(s.interval(order(), busy), BUSY_INTERVAL);
        assert!(!s.poll(Some(order()), busy, ms(9_999)));
        assert!(s.poll(Some(order()), busy, ms(10_000)));
        s.answered(ms(10_020));
        // the card holds a frame, is lost, or is out of memory: nothing
        let gone = Conditions { card_gone: true, ..busy };
        assert_eq!(s.state(Some(order()), gone), State::CardBusy);
        for at in [20_000, 60_000, 600_000] {
            assert!(!s.poll(Some(order()), gone, ms(at)));
        }
        assert!(!s.awaiting());
        // the card answers again: pictures come back
        assert!(s.poll(Some(order()), FINE, ms(600_001)));
    }

    #[test]
    fn nothing_is_asked_while_there_is_nothing_to_show() {
        let t0 = Instant::now();
        for (c, state) in [
            (Conditions { in_world: false, ..FINE }, State::NotInWorld),
            (Conditions { minimised: true, ..FINE }, State::Minimised),
            (Conditions { occupied: true, ..FINE }, State::Occupied),
            (
                Conditions {
                    in_world: false,
                    card_gone: true,
                    ..FINE
                },
                State::CardBusy,
            ),
        ] {
            let mut s = Schedule::default();
            assert!(!s.poll(Some(order()), c, t0), "{state:?}");
            assert!(!s.awaiting());
            assert_eq!(s.state(Some(order()), c), state);
            assert_eq!(s.state(None, c), State::Off);
        }
    }

    #[test]
    fn closing_a_monitor_forgets_the_lowered_rate() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        assert!(s.poll(Some(order()), FINE, t0));
        s.timed_out();
        assert_eq!(s.level(), 2);
        assert!(!s.poll(None, FINE, t0));
        assert_eq!(s.level(), 0);
        // a picture still awaited when the monitor closes is still awaited
        // (its answer is taken and dropped by the caller), then forgotten
        // with a renderer made again
        let mut s = Schedule::default();
        assert!(s.poll(Some(order()), FINE, t0));
        assert!(!s.poll(None, FINE, t0));
        assert!(s.awaiting());
        s.forget();
        assert!(!s.awaiting());
        // answers nobody awaits change nothing
        s.answered(t0);
        s.failed();
        s.timed_out();
        assert_eq!(s.level(), 0);
    }
}
