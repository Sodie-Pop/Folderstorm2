//! Video memory with several characters (`folderstorm/plans/gpu-memory.md`,
//! phases 2 to 4): what the host gives each character of the graphics
//! card, and the viewer's side of Economy. All pure.
//!
//! - **Shares** ([`shares`]): every character behind the active one may
//!   hold the user's "Background characters' video memory" in all
//!   (textures, geometry, render targets); the active character keeps the
//!   rest, leaving each background character the room up to its share
//!   ([`Memory::reserved_mb`]), as Aurora's own registry leaves the
//!   viewers in front theirs (`instances.rs`, `share_cap`). A background
//!   character counts as needing its share, not nothing.
//! - **Economy** ([`Release`]): a background character in Economy gives
//!   back every texture (the avatars' bakes too) and its 3D targets, then
//!   its window is minimised. Watched by a monitor it stays behind, as its
//!   pictures need, with a small texture budget: its pictures are blurrier.
//! - **The release before the minimise** ([`Releasing`]): a minimised
//!   window gets few frames or none, and the textures are given back by
//!   frames. The window is minimised once the release is done (or after
//!   [`RELEASE_WAIT`]), and while it waits the viewer draws a frame itself
//!   when none came for [`DRAW_AFTER`], and draws at [`RELEASE_FPS`] at
//!   least: the texture pages are emptied and destroyed a few per frame.
//!   A window that still has the keyboard is not minimised: the host's
//!   switch through the bar or a hotkey reaches it before the character
//!   brought forward has taken the keyboard, and it waits for that.
//! - **A monitor's picture** ([`monitor_viewport`]): a watched window
//!   draws its world at the picture's size, so that a picture never makes
//!   the window's full-size render targets again (twice per picture
//!   before, plan section 3.5).

use crate::ipc::{MAX_SLOTS, Memory, Release};
use crate::monitor;
use std::time::{Duration, Instant};

/// The default share of a background character, in megabytes: on a 12 GB
/// card four characters behind leave the active one about 8 GB; on an
/// 8 GB card, two leave it about 6 GB. A character behind in a busy region
/// holds more than this in geometry alone: its textures then keep their
/// floor (256 MB) and the rest of what it holds stays outside its share.
pub const DEFAULT_BACKGROUND_MB: u32 = 1024;
/// The choices of the setting, in megabytes.
pub const MIN_BACKGROUND_MB: u32 = 256;
pub const MAX_BACKGROUND_MB: u32 = 4096;
pub const BACKGROUND_STEP_MB: u32 = 128;
/// The share of an Economy character watched by a monitor, at most: its
/// pictures (640 × 360 at most) are drawn with lowered textures.
pub const WATCHED_ECONOMY_MB: u32 = 512;
/// The room left to the others is said in steps of this many megabytes:
/// the background characters' use moves a little at every report, and
/// each change would be an assignment sent again.
pub const RESERVE_STEP_MB: u32 = 256;

/// Longest wait for Economy's release before the window is minimised
/// anyway.
pub const RELEASE_WAIT: Duration = Duration::from_secs(5);
/// During the release, a frame is drawn by the viewer itself when the
/// window got none for this long.
pub const DRAW_AFTER: Duration = Duration::from_millis(500);
/// During the release, a window behind draws at least this many frames a
/// second (its 3D is paused: a frame is the interface and a sky at a
/// sixteenth of the size): the texture table empties at most one page per
/// 50 ms and destroys the emptied ones a few per frame, so the release
/// would otherwise last as long as the slow frames of a window behind.
pub const RELEASE_FPS: f32 = 20.0;
/// Frames drawn since the release began before it can be done: the first
/// one pauses the 3D, the next ones draw it with its targets at their
/// smallest.
const RELEASE_FRAMES: u64 = 2;

/// The setting as the host keeps it: within its choices, in its steps.
pub fn clamp_background(mb: u32) -> u32 {
    let mb = mb.clamp(MIN_BACKGROUND_MB, MAX_BACKGROUND_MB);
    (mb + BACKGROUND_STEP_MB / 2) / BACKGROUND_STEP_MB * BACKGROUND_STEP_MB
}

/// What the host knows of a running character for its share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Character {
    /// The active character.
    pub active: bool,
    /// Its standby, as chosen, is Economy.
    pub economy: bool,
    /// A monitor that shows it is on screen (its own, or the cycling
    /// monitor): its window stays behind for the pictures.
    pub watched: bool,
    /// Megabytes of video memory its viewer last said it holds (0: not
    /// said yet).
    pub held_mb: u32,
}

/// A background character's share and release: `setting` is the user's
/// share for the characters behind, in megabytes.
pub fn background(setting: u32, economy: bool, watched: bool) -> Memory {
    let setting = clamp_background(setting);
    match (economy, watched) {
        (false, _) => Memory {
            share_mb: setting,
            reserved_mb: 0,
            release: Release::Keep,
        },
        (true, true) => Memory {
            share_mb: setting.min(WATCHED_ECONOMY_MB),
            reserved_mb: 0,
            release: Release::Lowered,
        },
        (true, false) => Memory {
            share_mb: setting,
            reserved_mb: 0,
            release: Release::Full,
        },
    }
}

/// The room a background character may still take, in megabytes: up to
/// its share. Economy's full release takes nothing more while it waits
/// minimised: what it holds is already out of the driver's budget of the
/// others.
fn room(memory: Memory, held_mb: u32) -> u32 {
    match memory.release {
        Release::Full => 0,
        Release::Keep | Release::Lowered => memory.share_mb.saturating_sub(held_mb),
    }
}

/// Each running character's memory (None: no character, or not running):
/// the active one keeps what the others leave, they hold their share.
pub fn shares(setting: u32, characters: &[Option<Character>; MAX_SLOTS]) -> [Option<Memory>; MAX_SLOTS] {
    let mut memories = [None; MAX_SLOTS];
    let mut room_mb: u64 = 0;
    for (memory, c) in memories.iter_mut().zip(characters) {
        let Some(c) = c.filter(|c| !c.active) else {
            continue;
        };
        let m = background(setting, c.economy, c.watched);
        room_mb += u64::from(room(m, c.held_mb));
        *memory = Some(m);
    }
    let step = u64::from(RESERVE_STEP_MB);
    let reserved = room_mb.div_ceil(step) * step;
    for (memory, c) in memories.iter_mut().zip(characters) {
        if c.is_some_and(|c| c.active) {
            *memory = Some(Memory {
                share_mb: 0,
                reserved_mb: u32::try_from(reserved).unwrap_or(u32::MAX),
                release: Release::Keep,
            });
        }
    }
    memories
}

/// What a viewer does next about Economy's release ([`Releasing::poll`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// No release under way.
    Idle,
    /// Still releasing; `draw`: no frame came for a while, the viewer
    /// draws one itself.
    Wait { draw: bool },
    /// Done, or waited long enough: the window is minimised now.
    Minimise,
}

/// Economy's release before the window is minimised, in the viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Releasing {
    /// When it began, and the frame count then.
    began: Option<(Instant, u64)>,
    /// The last frame count seen, and when it was first seen.
    seen: Option<(u64, Instant)>,
}

impl Releasing {
    /// The host asked for the minimise: the release begins (`frame`: the
    /// viewer's frame count). Asked again while under way, it goes on.
    pub fn start(&mut self, now: Instant, frame: u64) {
        if self.began.is_none() {
            self.began = Some((now, frame));
            self.seen = Some((frame, now));
        }
    }

    /// The window is not minimised after all (the character came to the
    /// front, or is not in Economy any more).
    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    pub fn under_way(&self) -> bool {
        self.began.is_some()
    }

    /// At each turn of the event loop: `frame` the viewer's frame count,
    /// `released` whether its textures say they gave everything back,
    /// `keyboard` whether its window still has the keyboard: the release
    /// and its wait then start over once the keyboard has left, however
    /// long it stays (a window the user is in is never minimised under
    /// them, and it counts as in front for its textures until then).
    pub fn poll(&mut self, now: Instant, frame: u64, released: bool, keyboard: bool) -> Step {
        if self.began.is_none() {
            return Step::Idle;
        }
        if keyboard {
            self.began = Some((now, frame));
            self.seen = Some((frame, now));
            return Step::Wait { draw: false };
        }
        let Some((since, first)) = self.began else {
            return Step::Idle;
        };
        let drawn = frame.saturating_sub(first);
        if (released && drawn >= RELEASE_FRAMES) || now.saturating_duration_since(since) >= RELEASE_WAIT {
            self.cancel();
            return Step::Minimise;
        }
        let seen = match self.seen {
            Some((count, at)) if count == frame => at,
            _ => {
                self.seen = Some((frame, now));
                now
            }
        };
        let draw = now.saturating_duration_since(seen) >= DRAW_AFTER;
        if draw {
            // (one asked: the next is asked after another wait)
            self.seen = Some((frame, now));
        }
        Step::Wait { draw }
    }
}

/// Where a watched background window draws its world (`base`: the world's
/// part of the window, `[x, y, width, height]`): a rectangle of the
/// monitor's picture size in its middle, the picture then drawn at the
/// size the window's targets already have. None when the picture is as
/// large as the world (nothing to gain) or the world has no size.
pub fn monitor_viewport(base: [u32; 4], size: monitor::Size) -> Option<[u32; 4]> {
    let [x, y, w, h] = base;
    let (pw, ph) = monitor::fit(size, (w, h))?;
    if pw >= w && ph >= h {
        return None;
    }
    let (pw, ph) = (pw.min(w), ph.min(h));
    Some([x + (w - pw) / 2, y + (h - ph) / 2, pw, ph])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(active: bool, economy: bool, watched: bool, held_mb: u32) -> Option<Character> {
        Some(Character {
            active,
            economy,
            watched,
            held_mb,
        })
    }

    #[test]
    fn the_setting_keeps_to_its_choices() {
        assert_eq!(clamp_background(0), MIN_BACKGROUND_MB);
        assert_eq!(clamp_background(100_000), MAX_BACKGROUND_MB);
        assert_eq!(clamp_background(1000), 1024);
        assert_eq!(clamp_background(1100), 1152);
        assert_eq!(clamp_background(DEFAULT_BACKGROUND_MB), DEFAULT_BACKGROUND_MB);
        for mb in [0, 300, 777, 2049, 5000] {
            let kept = clamp_background(mb);
            assert_eq!(kept % BACKGROUND_STEP_MB, 0, "{mb}");
            assert!((MIN_BACKGROUND_MB..=MAX_BACKGROUND_MB).contains(&kept), "{mb}");
        }
    }

    #[test]
    fn a_background_character_holds_its_share_and_economy_releases() {
        let warm = background(1024, false, false);
        assert_eq!((warm.share_mb, warm.release), (1024, Release::Keep));
        assert_eq!(background(1024, false, true), warm, "a watched warm character is as any warm one");
        let economy = background(1024, true, false);
        assert_eq!((economy.share_mb, economy.release), (1024, Release::Full));
        let watched = background(1024, true, true);
        assert_eq!((watched.share_mb, watched.release), (WATCHED_ECONOMY_MB, Release::Lowered));
        // a share smaller than the watched one's is kept
        assert_eq!(background(256, true, true).share_mb, 256);
    }

    #[test]
    fn the_active_character_leaves_the_others_the_room_up_to_their_share() {
        let mut list = [None; MAX_SLOTS];
        list[0] = character(true, false, false, 6000);
        // warm, holding 300 MB of its 1024: 724 left
        list[1] = character(false, false, false, 300);
        // warm, holding more than its share: nothing left
        list[2] = character(false, false, false, 1500);
        // economy, released and minimised: nothing more
        list[3] = character(false, true, false, 400);
        // watched economy, 100 MB of its 512: 412 left
        list[4] = character(false, true, true, 100);
        let m = shares(1024, &list);
        let active = m[0].expect("the active one");
        assert_eq!(active.share_mb, 0, "no share: it keeps the rest");
        // 724 + 412 = 1136, said in steps of 256
        assert_eq!(active.reserved_mb, 1280);
        assert_eq!(active.release, Release::Keep);
        assert_eq!(m[1].map(|m| m.release), Some(Release::Keep));
        assert_eq!(m[3].map(|m| m.release), Some(Release::Full));
        assert_eq!(m[4].map(|m| m.release), Some(Release::Lowered));
        for m in m.iter().skip(1) {
            assert_eq!(m.map(|m| m.reserved_mb), Some(0));
        }
    }

    #[test]
    fn a_character_that_has_not_said_what_it_holds_counts_its_whole_share() {
        let mut list = [None; MAX_SLOTS];
        list[1] = character(true, false, false, 0);
        list[3] = character(false, false, false, 0);
        let m = shares(768, &list);
        assert_eq!(m[1].map(|m| m.reserved_mb), Some(768));
        assert_eq!(m[0], None);
        assert_eq!(m[2], None);
        // alone, nothing is left aside
        let mut alone = [None; MAX_SLOTS];
        alone[0] = character(true, true, false, 100);
        assert_eq!(shares(1024, &alone)[0].map(|m| m.reserved_mb), Some(0));
        // no active character: the others still get their share
        let mut none = [None; MAX_SLOTS];
        none[2] = character(false, true, false, 0);
        assert_eq!(shares(1024, &none)[2].map(|m| m.release), Some(Release::Full));
    }

    #[test]
    fn the_room_moves_in_steps_and_a_small_change_says_nothing_new() {
        let mut list = [None; MAX_SLOTS];
        list[0] = character(true, false, false, 4000);
        list[1] = character(false, false, false, 600);
        let first = shares(1024, &list)[0];
        list[1] = character(false, false, false, 630);
        assert_eq!(shares(1024, &list)[0], first, "30 MB more held: the same room said");
    }

    #[test]
    fn economy_minimises_once_released_or_after_its_wait() {
        let t0 = Instant::now();
        let mut r = Releasing::default();
        assert_eq!(r.poll(t0, 0, true, false), Step::Idle);
        r.start(t0, 10);
        assert!(r.under_way());
        // released but no frame drawn since: not yet (the targets shrink
        // with the frames)
        assert_eq!(r.poll(t0, 10, true, false), Step::Wait { draw: false });
        assert_eq!(r.poll(t0 + Duration::from_millis(50), 11, true, false), Step::Wait { draw: false });
        assert_eq!(r.poll(t0 + Duration::from_millis(100), 12, true, false), Step::Minimise);
        assert!(!r.under_way());
        assert_eq!(r.poll(t0 + Duration::from_millis(120), 13, true, false), Step::Idle);
        // never released: minimised after the wait all the same
        r.start(t0, 0);
        assert_eq!(r.poll(t0 + Duration::from_secs(1), 40, false, false), Step::Wait { draw: false });
        assert_eq!(r.poll(t0 + RELEASE_WAIT, 41, false, false), Step::Minimise);
    }

    #[test]
    fn a_window_with_the_keyboard_is_minimised_once_it_lost_it() {
        // the switch came through the bar of this very window: the host's
        // Minimise reached it before the other character took the keyboard
        let t0 = Instant::now();
        let mut r = Releasing::default();
        r.start(t0, 0);
        assert_eq!(r.poll(t0 + Duration::from_millis(100), 3, true, true), Step::Wait { draw: false });
        let left = t0 + RELEASE_WAIT * 3;
        assert_eq!(r.poll(left, 40, false, true), Step::Wait { draw: false });
        assert!(r.under_way(), "it goes on");
        // the keyboard left: the release starts from there (its frames, its
        // wait)
        assert_eq!(r.poll(left, 40, true, false), Step::Wait { draw: false });
        assert_eq!(r.poll(left + Duration::from_millis(100), 42, true, false), Step::Minimise);
        // never released once the keyboard left: after the wait from there
        r.start(t0, 0);
        assert_eq!(r.poll(left, 40, false, true), Step::Wait { draw: false });
        assert_eq!(r.poll(left + RELEASE_WAIT / 2, 41, false, false), Step::Wait { draw: false });
        assert_eq!(r.poll(left + RELEASE_WAIT, 42, false, false), Step::Minimise);
    }

    #[test]
    fn a_window_without_frames_gets_them_from_the_viewer() {
        let t0 = Instant::now();
        let mut r = Releasing::default();
        r.start(t0, 5);
        assert_eq!(r.poll(t0 + Duration::from_millis(100), 5, false, false), Step::Wait { draw: false });
        assert_eq!(r.poll(t0 + DRAW_AFTER, 5, false, false), Step::Wait { draw: true });
        // asked once: the next one after another wait
        assert_eq!(
            r.poll(t0 + DRAW_AFTER + Duration::from_millis(10), 5, false, false),
            Step::Wait { draw: false }
        );
        assert_eq!(r.poll(t0 + DRAW_AFTER * 2, 5, false, false), Step::Wait { draw: true });
        // frames come: none asked
        assert_eq!(r.poll(t0 + DRAW_AFTER * 3, 6, false, false), Step::Wait { draw: false });
        // a second start while under way changes nothing
        r.start(t0 + Duration::from_secs(1), 99);
        assert_eq!(r.poll(t0 + RELEASE_WAIT, 7, false, false), Step::Minimise);
        // cancelled: nothing to minimise
        r.start(t0, 0);
        r.cancel();
        assert_eq!(r.poll(t0 + RELEASE_WAIT, 7, true, false), Step::Idle);
    }

    #[test]
    fn a_watched_window_draws_its_world_at_the_picture_s_size() {
        use monitor::Size;
        // a full HD window, a large monitor: 640 × 360 in the middle
        assert_eq!(monitor_viewport([0, 0, 1920, 1080], Size::Large), Some([640, 360, 640, 360]));
        // under the docked bar: inside the world's part
        assert_eq!(monitor_viewport([0, 40, 1920, 1040], Size::Small), Some([800, 474, 320, 172]));
        // the picture is taken at the size of that rectangle, whatever the
        // monitor's size and the window's shape
        for size in [Size::Small, Size::Medium, Size::Large] {
            for base in [
                [0, 0, 1920, 1080],
                [0, 0, 3440, 1440],
                [0, 0, 1080, 1920],
                [10, 30, 1366, 700],
                [0, 0, 801, 599],
            ] {
                let rect = monitor_viewport(base, size).expect("smaller than the window");
                let picture = monitor::fit(size, (base[2], base[3]));
                assert_eq!(monitor::fit(size, (rect[2], rect[3])), picture, "{size:?} {base:?}");
                assert_eq!(Some((rect[2], rect[3])), picture, "{size:?} {base:?}");
                assert!(rect[0] + rect[2] <= base[0] + base[2] && rect[1] + rect[3] <= base[1] + base[3]);
            }
        }
        // a window no larger than the picture: nothing to gain
        assert_eq!(monitor_viewport([0, 0, 640, 360], Size::Large), None);
        assert_eq!(monitor_viewport([0, 0, 0, 0], Size::Large), None);
    }
}
