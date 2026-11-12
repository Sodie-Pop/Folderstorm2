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
//! file, a log or the clipboard, and no earlier picture is kept. The
//! picture stays here while a window shows it, so that the monitors'
//! own window (checkpoint 6) and the characters' cards can take turns
//! at drawing it without waiting for the next one.

use super::Host;
use ::folderstorm_session::host::Watch;
use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::monitor;
use std::time::Instant;

/// What the host keeps of one character's monitor.
#[derive(Default)]
pub(super) struct Screen {
    /// The latest picture: the window that draws it makes its texture of
    /// it (checkpoint 6: that window can change while the picture stays).
    pub picture: Option<std::sync::Arc<egui::ColorImage>>,
    /// Which picture that is: one more with each picture, so that a
    /// window knows the one its texture holds.
    pub serial: u64,
    /// The picture's size in pixels and when it came. None: no picture.
    pub shown: Option<Shown>,
}

impl Screen {
    /// Nothing is kept of the picture. (The count goes on: the next
    /// picture is never taken for this one.)
    fn drop_picture(&mut self) {
        self.picture = None;
        self.shown = None;
    }

    fn put(&mut self, image: egui::ColorImage, now: Instant) {
        self.serial += 1;
        self.shown = Some(Shown { size: image.size, at: now });
        self.picture = Some(std::sync::Arc::new(image));
    }
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
                screen.drop_picture();
                continue;
            }
            if let Some((launch, shot)) = shot
                && self.desk.frame_wanted(slot, launch, &shot.header)
            {
                screen.put(shot.image, now);
            }
        }
    }

    /// Hands a picture to the host as its reader thread would, for the
    /// window's tests.
    #[cfg(test)]
    pub(super) fn test_picture(&mut self, slot: usize, size: [usize; 2], fill: egui::Color32, now: Instant) {
        self.screens[slot].put(egui::ColorImage::filled(size, fill), now);
    }
}
