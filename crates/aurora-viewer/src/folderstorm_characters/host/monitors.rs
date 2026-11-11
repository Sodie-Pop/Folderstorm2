//! Checkpoint 5 of the session host, monitors (session-host plan,
//! section 12): what the host keeps of the pictures its background
//! characters send. The rules are the `folderstorm-session` crate's
//! (`monitor`, the desk's `watch` and `frame_wanted`); the pictures are
//! read in [`super::process`], the window's side is `ui/monitors.rs`.
//!
//! For each character at most one picture is kept: the latest, for as
//! long as its monitor is open and its viewer runs in the background. It
//! is dropped as soon as the monitor closes, the character comes to the
//! front, leaves the world or its viewer ends. Nothing is written to a
//! file, a log or the clipboard, and no earlier picture is kept.

use super::Host;
use ::folderstorm_session::host::Watch;
use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::monitor;
use std::time::Instant;

/// What the host keeps of one character's monitor.
#[derive(Default)]
pub(super) struct Screen {
    /// The latest picture, until the window has taken it for its texture.
    pub fresh: Option<egui::ColorImage>,
    /// The picture the window shows (or is about to): its size in pixels
    /// and when it came. None: no picture.
    pub shown: Option<Shown>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    pub size: [usize; 2],
    pub at: Instant,
}

impl Host {
    /// The pictures that came since the last look: the latest of each
    /// viewer is kept when its monitor asks for it, the others are
    /// dropped; a monitor that is not watching any more keeps nothing.
    pub(super) fn tick_monitors(&mut self, now: Instant) {
        for slot in 0..MAX_SLOTS {
            // (taken even when it will be dropped: nothing waits there)
            let shot = self.viewers[slot]
                .as_ref()
                .and_then(|viewer| viewer.shot().map(|shot| (viewer.launch, shot)));
            let left_the_world = self
                .desk
                .slot(slot)
                .and_then(|s| s.status.as_ref())
                .is_some_and(|status| status.monitor == monitor::State::NotInWorld);
            let keeps = match self.desk.watch(slot) {
                Watch::Closed | Watch::NoViewer | Watch::InFront => false,
                Watch::Hidden | Watch::TooMany | Watch::Asked { .. } => !left_the_world,
            };
            let screen = &mut self.screens[slot];
            if !keeps {
                *screen = Screen::default();
                continue;
            }
            if let Some((launch, shot)) = shot
                && self.desk.frame_wanted(slot, launch, &shot.header)
            {
                screen.shown = Some(Shown {
                    size: shot.image.size,
                    at: now,
                });
                screen.fresh = Some(shot.image);
            }
        }
    }

    /// Hands a picture to the host as its reader thread would, for the
    /// window's tests.
    #[cfg(test)]
    pub(super) fn test_picture(&mut self, slot: usize, size: [usize; 2], fill: egui::Color32, now: Instant) {
        self.screens[slot] = Screen {
            fresh: Some(egui::ColorImage::filled(size, fill)),
            shown: Some(Shown { size, at: now }),
        };
    }
}
