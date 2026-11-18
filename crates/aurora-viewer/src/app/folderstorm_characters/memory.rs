//! Folderstorm: a managed viewer's side of the video memory plan
//! (`folderstorm/plans/gpu-memory.md`, phases 2 to 4; the rules are
//! `folderstorm_session::memory`, the textures' side is
//! `scene/folderstorm_textures.rs`).
//!
//! - **The share** the host gives goes to the textures as soon as the
//!   assignment comes, from the event loop's pump, not from a frame.
//! - **Economy** is released before the window is minimised: the host's
//!   Minimise starts the release (every texture to its last level, the 3D
//!   paused with its targets at their smallest), and the window is
//!   minimised once it is done or after `memory::RELEASE_WAIT`. While it
//!   waits, the pump draws a frame itself when the window got none for a
//!   while: the release never depends on a minimised window's rare frames.
//!   Coming back, the textures and the faces load again (a second or two).
//! - **A monitor's picture** (`world_viewport`'s monitor rectangle): a
//!   watched window draws its world at the picture's size, so a picture
//!   never makes the window's full-size render targets again.

use super::*;
use crate::scene::textures::folderstorm_memory::Limits;
use ::folderstorm_session::ipc::Release;
use ::folderstorm_session::memory::{self, Step};

impl App {
    /// The host's word on the video memory, as soon as an assignment is
    /// taken (or the host is gone: none). A release under way that is not
    /// Economy's any more is called off.
    pub(super) fn folderstorm_memory_assign(&mut self) {
        let memory = self.folderstorm_characters.managed.memory();
        let state = &mut self.folderstorm_characters.memory;
        if std::mem::replace(&mut state.applied, memory) != memory {
            self.scene.textures.set_folderstorm_memory(Limits::from_host(memory));
        }
        if memory.release != Release::Full && state.releasing.under_way() {
            log::info!("session memory: not minimised after all (no longer in Economy)");
            state.releasing.cancel();
        }
    }

    /// The host asks for the minimise (Economy). Never the active
    /// character, never a window with the keyboard. With Economy's release
    /// it waits for the release; otherwise (an older host) at once, as
    /// before.
    pub(super) fn folderstorm_memory_minimise(&mut self) {
        if self.folderstorm_in_front() {
            return;
        }
        if self.folderstorm_characters.managed.memory().release != Release::Full {
            if let Some(g) = &self.gfx {
                g.window.set_minimized(true);
            }
            return;
        }
        let (frame, passes) = (self.frame_count, self.scene.textures.wants_passes());
        let state = &mut self.folderstorm_characters.memory;
        if !state.releasing.under_way() {
            state.since_passes = passes;
            state.began = Some(Instant::now());
            log::info!("session memory: Economy, giving the video memory back before the window is minimised");
        }
        state.releasing.start(Instant::now(), frame);
    }

    /// Every turn of the event loop: Economy's release under way, its own
    /// frame when the window gets none, and the minimise once it is done.
    pub(super) fn folderstorm_memory_pump(&mut self, event_loop: &ActiveEventLoop) {
        if !self.folderstorm_characters.memory.releasing.under_way() {
            return;
        }
        if self.folderstorm_in_front() || self.gfx.is_none() {
            if self.gfx.is_some() {
                log::info!("session memory: not minimised after all (the window is in front)");
            }
            self.folderstorm_characters.memory.releasing.cancel();
            return;
        }
        let since = self.folderstorm_characters.memory.since_passes;
        let released = self.scene.textures.folderstorm_released(since);
        let now = Instant::now();
        match self.folderstorm_characters.memory.releasing.poll(now, self.frame_count, released) {
            Step::Idle => {}
            Step::Wait { draw: false } => {}
            Step::Wait { draw: true } => self.frame(event_loop),
            Step::Minimise => {
                let waited = self.folderstorm_characters.memory.began.take().map(|at| now.duration_since(at));
                let stats = &self.scene.textures.stats;
                log::info!(
                    "session memory: {} after {} ms ({} MB of video memory, texture target {} MB), the window is minimised",
                    if released { "released" } else { "not all released" },
                    waited.unwrap_or_default().as_millis(),
                    self.scene.textures.video_usage() >> 20,
                    stats.target_bytes >> 20,
                );
                if let Some(g) = &self.gfx {
                    g.window.set_minimized(true);
                }
            }
        }
    }

    /// For the host's status: the video memory the driver counts for this
    /// viewer, in megabytes (0: not known).
    pub(super) fn folderstorm_vram_mb(&self) -> u32 {
        u32::try_from(self.scene.textures.video_usage() >> 20).unwrap_or(u32::MAX)
    }

    /// A watched background window draws its world at its monitor's
    /// picture size, in the middle of `base` (the world's part of a window
    /// of `size`, None for all of it): each picture is then drawn without
    /// making the window's full-size render targets, and between pictures
    /// the paused 3D holds a sixteenth of that small rectangle. None when
    /// no picture is asked of this window.
    pub(in crate::app) fn folderstorm_monitor_viewport(&self, size: (u32, u32), base: Option<[u32; 4]>) -> Option<[u32; 4]> {
        if !self.in_world() {
            return None;
        }
        let order = self.folderstorm_characters.managed.monitor(self.folderstorm_focused())?;
        memory::monitor_viewport(base.unwrap_or([0, 0, size.0, size.1]), order.size)
    }
}
