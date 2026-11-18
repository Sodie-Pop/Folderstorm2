//! Folderstorm: the session host's share of the graphics card in the
//! texture streamer (`folderstorm/plans/gpu-memory.md`, phases 2 and 4;
//! the host's rules are `folderstorm_session::memory`). A child of
//! `scene/textures.rs`, which reaches it through three marks in `press`:
//!
//! - **the share** lowers the textures' target as Aurora's own limits do
//!   (`texture_budget::relieve`): a background character's whole process
//!   holds its share, the textures what the rest leaves (never under
//!   [`MIN_TEXTURES`]); the active character counts its driver budget
//!   less what the others may still take, through Aurora's own target
//!   (`texture_budget::texture_target`);
//! - **Economy** gives the textures back as a window behind past the
//!   driver's budget does (`texture_budget::Background`, "crowded"): every
//!   texture to its last level, the avatars' bakes too, and the 3D drawing
//!   paused (its targets at their smallest);
//! - **a watched Economy character** holds the highest pressure step at
//!   once, as a minimised window does after its second, and keeps the 3D
//!   for its monitor's pictures: they are drawn with lowered textures.
//!
//! Only ever lowers: without a host, or for the active character with
//! nobody behind, every limit here is none and Aurora's rules stand alone.

use super::TextureStreamer;
use super::texture_budget;
use ::folderstorm_session::ipc::{Memory, Release};
use aurora_render::vram::VideoMemory;
use std::time::Instant;

const MB: u64 = 1 << 20;

/// The textures keep at least this much under a share (Aurora's own
/// floor, `texture_budget`'s minimum).
pub const MIN_TEXTURES: u64 = 256 * MB;

/// What the session host asks of this viewer's video memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Bytes the whole process may hold; None: no share.
    pub share: Option<u64>,
    /// Bytes of the driver's budget left to the other characters.
    pub reserved: u64,
    pub release: Release,
}

impl Limits {
    /// The host's word, in bytes.
    pub fn from_host(memory: Memory) -> Self {
        Self {
            share: (memory.share_mb > 0).then(|| u64::from(memory.share_mb) * MB),
            reserved: u64::from(memory.reserved_mb) * MB,
            release: memory.release,
        }
    }

    /// Economy: everything is given back, as if the memory were short.
    pub fn economy(&self) -> bool {
        self.release == Release::Full
    }

    /// A watched Economy character: the highest step at once.
    pub fn held(&self) -> bool {
        self.release == Release::Lowered
    }

    /// The textures' limit with `relief` (Aurora's own, gpu_load.rs) and
    /// the share: the lower of them, None when neither limits. `pages`:
    /// the bytes of the texture pages now; `video`: the driver's figures.
    pub fn cap(&self, relief: Option<u64>, pages: u64, video: Option<VideoMemory>) -> Option<u64> {
        let mut cap = relief;
        let mut lower = |limit: u64| cap = Some(cap.map_or(limit, |c| c.min(limit)));
        if let Some(share) = self.share {
            // what the rest of the process holds: render targets, geometry,
            // allocator room (as texture_budget::texture_target counts it)
            let others = video.map_or(0, |v| v.usage.max(v.allocated).saturating_sub(pages));
            lower(share.saturating_sub(others).max(MIN_TEXTURES));
        }
        if self.reserved > 0
            && let Some(v) = video
        {
            let left = VideoMemory {
                budget: v.budget.saturating_sub(self.reserved),
                ..v
            };
            lower(texture_budget::texture_target(u64::MAX, pages, Some(left)));
        }
        cap
    }
}

impl TextureStreamer {
    /// The session host's word, as soon as it comes (not at a frame: a
    /// minimised window has few). A change of the release while the window
    /// is behind starts its background state over: the pause Economy
    /// latched does not outlive it (a monitor opened on an Economy
    /// character draws its pictures), and the textures follow the new
    /// release from the next budget pass.
    pub fn set_folderstorm_memory(&mut self, limits: Limits) {
        let was = std::mem::replace(&mut self.folderstorm, limits);
        if was == limits {
            return;
        }
        match limits.release {
            Release::Full => log::info!(
                "session memory: Economy, every texture goes to its last level (the avatars' bakes too) and the 3D pauses until this character is active"
            ),
            Release::Lowered => log::info!(
                "session memory: Economy watched by a monitor, the textures are lowered at once and kept under a share of {} MB",
                limits.share.unwrap_or(0) >> 20
            ),
            Release::Keep => match limits.share {
                Some(share) => log::info!("session memory: a background character, {} MB of video memory in all", share >> 20),
                None if limits.reserved > 0 => log::info!(
                    "session memory: the active character, {} MB left to the characters behind",
                    limits.reserved >> 20
                ),
                None => {}
            },
        }
        if was.release != limits.release && was.release != Release::Keep && !(self.in_front && !self.minimized) {
            self.background = texture_budget::Background::default();
            self.pressure.release(Instant::now());
        }
    }

    /// Economy has given back what it can: the 3D is paused, the textures
    /// are held at the highest step and none waits to be lowered, at least
    /// two passes after `since_passes` (`wants_passes` when it began).
    pub fn folderstorm_released(&self, since_passes: u64) -> bool {
        self.folderstorm.economy()
            && self.background.paused()
            && self.pressure.held()
            && self.wants_passes >= since_passes + 2
            && self.reductions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const GB: u64 = 1 << 30;

    fn video(budget: u64, usage: u64) -> Option<VideoMemory> {
        Some(VideoMemory {
            budget,
            usage,
            allocated: usage,
            shared_budget: 0,
        })
    }

    #[test]
    fn without_a_host_nothing_is_limited() {
        let none = Limits::default();
        assert_eq!(none.cap(None, GB, video(8 * GB, 3 * GB)), None);
        assert_eq!(
            none.cap(Some(2 * GB), GB, video(8 * GB, 3 * GB)),
            Some(2 * GB),
            "Aurora's relief stands"
        );
        assert_eq!(Limits::from_host(Memory::default()), none);
        assert!(!none.economy() && !none.held());
    }

    #[test]
    fn a_background_share_leaves_the_textures_what_the_rest_does_not_hold() {
        let warm = Limits::from_host(Memory {
            share_mb: 1024,
            reserved_mb: 0,
            release: Release::Keep,
        });
        assert_eq!(warm.share, Some(GB));
        // 600 MB of pages, 1 GB held in all: 424 MB of the rest, 600 left
        assert_eq!(warm.cap(None, 600 * MB, video(8 * GB, GB)), Some(600 * MB));
        // the rest alone holds more than the share: the floor
        assert_eq!(warm.cap(None, 600 * MB, video(8 * GB, 2 * GB)), Some(MIN_TEXTURES));
        // the lower of the share and Aurora's relief
        assert_eq!(warm.cap(Some(300 * MB), 600 * MB, video(8 * GB, GB)), Some(300 * MB));
        // no reading of the driver: the share itself
        assert_eq!(warm.cap(None, 600 * MB, None), Some(GB));
    }

    #[test]
    fn the_active_character_leaves_the_others_their_room() {
        let active = Limits::from_host(Memory {
            share_mb: 0,
            reserved_mb: 2048,
            release: Release::Keep,
        });
        // 10 GB budget less 2 GB left aside: Aurora's target on 8 GB
        // (min(8 - 0.5, 8 * 0.8) = 6.4 GB) less the 1 GB the rest holds
        let cap = active.cap(None, 2 * GB, video(10 * GB, 3 * GB)).expect("limited");
        let alone = texture_budget::texture_target(u64::MAX, 2 * GB, video(10 * GB, 3 * GB));
        let expected = texture_budget::texture_target(u64::MAX, 2 * GB, video(8 * GB, 3 * GB));
        assert_eq!(cap, expected);
        assert!(cap < alone, "{cap} {alone}");
        // nothing left aside: no limit of its own
        let free = Limits::from_host(Memory::default());
        assert_eq!(free.cap(None, 2 * GB, video(10 * GB, 3 * GB)), None);
    }

    #[test]
    fn economy_and_its_watched_form_say_how_the_textures_go() {
        let full = Limits::from_host(Memory {
            share_mb: 1024,
            reserved_mb: 0,
            release: Release::Full,
        });
        assert!(full.economy() && !full.held());
        let lowered = Limits::from_host(Memory {
            share_mb: 512,
            reserved_mb: 0,
            release: Release::Lowered,
        });
        assert!(lowered.held() && !lowered.economy());
    }

    #[test]
    fn economy_s_pause_ends_with_it_while_the_window_is_behind() {
        let mut s = TextureStreamer::new(PathBuf::new());
        let t0 = Instant::now();
        s.set_window_state(false, false);
        let full = Limits {
            share: Some(GB),
            reserved: 0,
            release: Release::Full,
        };
        s.set_folderstorm_memory(full);
        // as `press` does with Economy: crowded, held at the highest step
        assert!(s.background.update(t0, false, false, true));
        s.pressure.hold_max(t0);
        assert!(s.drawing_paused());
        assert!(!s.folderstorm_released(0), "no pass yet");
        s.wants_passes = 2;
        assert!(s.folderstorm_released(0));
        s.reductions.push((uuid::Uuid::nil(), 9, 1));
        assert!(!s.folderstorm_released(0), "a texture still to lower");
        s.reductions.clear();
        // the same word again changes nothing
        s.set_folderstorm_memory(full);
        assert!(s.drawing_paused());
        // a monitor opened on it: the pause goes, its pictures can be drawn
        s.set_folderstorm_memory(Limits {
            share: Some(512 * MB),
            reserved: 0,
            release: Release::Lowered,
        });
        assert!(!s.drawing_paused());
        assert!(!s.pressure.held(), "the next pass holds it again");
        assert!(!s.folderstorm_released(0), "not Economy's full release any more");
    }

    #[test]
    fn in_front_the_background_state_is_aurora_s_own() {
        let mut s = TextureStreamer::new(PathBuf::new());
        let t0 = Instant::now();
        s.set_window_state(false, false);
        s.set_folderstorm_memory(Limits {
            release: Release::Full,
            ..Limits::default()
        });
        assert!(s.background.update(t0, false, false, true));
        // in front again (the active character): Aurora's own rule ends it
        s.set_window_state(true, false);
        s.set_folderstorm_memory(Limits::default());
        assert!(s.drawing_paused(), "left to `press`, which sees the window in front");
        assert!(!s.background.update(t0, true, false, false));
        assert!(!s.drawing_paused());
    }
}
