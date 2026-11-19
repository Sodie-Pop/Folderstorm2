//! Folderstorm: a managed viewer's side of the video memory plan
//! (`folderstorm/plans/gpu-memory.md`, phases 2 to 4 and "Economy, second
//! pass"; the rules are `folderstorm_session::memory`, the textures' side
//! is `scene/folderstorm_textures.rs`).
//!
//! - **The share** the host gives goes to the textures as soon as the
//!   assignment comes, from the event loop's pump, not from a frame.
//! - **Economy** is released before the window is minimised: the host's
//!   Minimise starts the release (every texture to its last level, the 3D
//!   paused with its targets at their smallest); once the 3D is paused the
//!   GPU culling buffers go back to their first size and the reflection
//!   probes and the shadow atlas to their smallest
//!   (`folderstorm_characters::economy_render`, the settings untouched).
//!   The window is minimised once the textures are done, the texture
//!   table has emptied and destroyed its pages and the pages have not
//!   dropped for `memory::PLATEAU`, or after `memory::RELEASE_WAIT`; the
//!   pages are logged again `memory::PAGES_AFTER_MINIMISE` later. While it waits the window draws at
//!   `memory::RELEASE_FPS` at least (`folderstorm_release_cap`), and the
//!   pump draws a frame itself when the window got none for a while: the
//!   release never depends on a minimised window's rare frames.
//! - **The keyboard.** A switch through the docked bar of this very window,
//!   or a hotkey, sends the Minimise while this window still has the
//!   keyboard (the character brought forward takes it a moment later).
//!   The release begins at once and the minimise waits for the keyboard
//!   to leave; only the active character's Minimise is ignored.
//! - **Coming back**: in front, the textures and the faces load again (a
//!   second or two), the probes and the shadow atlas are made again and
//!   the probes captured again. Watched again by a monitor while
//!   minimised, the window comes back behind, without the keyboard.
//! - **A monitor's picture** (`world_viewport`'s monitor rectangle): a
//!   watched window draws its world at the picture's size, so a picture
//!   never makes the window's full-size render targets again.

use super::*;
use crate::folderstorm_characters::{ReleaseFigures, economy_render};
use crate::scene::textures::folderstorm_memory::Limits;
use ::folderstorm_session::ipc::{Release, Role};
use ::folderstorm_session::memory::{self, Step};

impl App {
    /// The host's word on the video memory, as soon as an assignment is
    /// taken (or the host is gone: none). A release under way that is not
    /// Economy's any more is called off, and the lighter renderer ends.
    pub(super) fn folderstorm_memory_assign(&mut self) {
        let memory = self.folderstorm_characters.managed.memory();
        let state = &mut self.folderstorm_characters.memory;
        if std::mem::replace(&mut state.applied, memory) != memory {
            self.scene.textures.set_folderstorm_memory(Limits::from_host(memory));
        }
        if memory.release != Release::Full {
            if state.releasing.under_way() {
                log::info!("session memory: not minimised after all (no longer in Economy)");
                state.releasing.cancel();
            }
            self.folderstorm_memory_restore();
        }
        // a monitor shows this character again: back behind for its pictures
        if memory.release == Release::Lowered
            && std::mem::take(&mut self.folderstorm_characters.memory.minimised_by_host)
            && let Some(g) = self.gfx.as_ref().filter(|g| g.window.is_minimized() == Some(true))
        {
            log::info!("session memory: a monitor shows this character again, the window comes back behind");
            crate::folderstorm_characters::foreground::restore_behind(&g.window);
        }
    }

    /// The host asks for the minimise (Economy). Never the active
    /// character. With Economy's release it waits for the release (and for
    /// the keyboard to leave this window); otherwise (an older host) at
    /// once, as before, and never a window with the keyboard.
    pub(super) fn folderstorm_memory_minimise(&mut self) {
        if self.folderstorm_characters.managed.role() == Some(Role::Active) {
            return;
        }
        if self.folderstorm_characters.managed.memory().release != Release::Full {
            if !self.folderstorm_in_front()
                && let Some(g) = &self.gfx
            {
                g.window.set_minimized(true);
                self.folderstorm_characters.memory.minimised_by_host = true;
            }
            return;
        }
        let (frame, passes) = (self.frame_count, self.scene.textures.wants_passes());
        let figures = self.folderstorm_release_figures();
        let keyboard = self.folderstorm_focused();
        let state = &mut self.folderstorm_characters.memory;
        if !state.releasing.under_way() {
            state.since_passes = passes;
            state.pages.reset();
            state.began = Some(Instant::now());
            state.before = figures;
            log::info!(
                "session memory: Economy, giving the video memory back before the window is minimised{}",
                if keyboard {
                    " (it still has the keyboard: minimised once the keyboard leaves it)"
                } else {
                    ""
                }
            );
        }
        state.releasing.start(Instant::now(), frame);
    }

    /// Every turn of the event loop: Economy's release under way, the
    /// lighter renderer once the 3D is paused, its own frame when the
    /// window gets none, and the minimise once it is done.
    pub(super) fn folderstorm_memory_pump(&mut self, event_loop: &ActiveEventLoop) {
        // (the user went to it: no longer the host's to bring back)
        let keyboard = self.folderstorm_focused();
        if keyboard {
            self.folderstorm_characters.memory.minimised_by_host = false;
        }
        self.folderstorm_memory_pages_later();
        if !self.folderstorm_characters.memory.releasing.under_way() {
            return;
        }
        if self.folderstorm_characters.managed.role() == Some(Role::Active) || self.gfx.is_none() {
            if self.gfx.is_some() {
                log::info!("session memory: not minimised after all (the window is in front)");
            }
            self.folderstorm_characters.memory.releasing.cancel();
            self.folderstorm_memory_restore();
            return;
        }
        if !self.folderstorm_characters.memory.lowered && self.scene.textures.drawing_paused() {
            self.folderstorm_memory_lower();
        }
        if keyboard {
            // (in front for the textures while it has the keyboard: their
            // release, and its count of passes, start once it leaves)
            self.folderstorm_characters.memory.since_passes = self.scene.textures.wants_passes();
            self.folderstorm_characters.memory.pages.reset();
        }
        let now = Instant::now();
        let since = self.folderstorm_characters.memory.since_passes;
        let (settled, pages) = self.gfx.as_ref().map_or((false, 0), |g| {
            (g.renderer.textures.folderstorm_settled(), g.renderer.textures.page_bytes())
        });
        // (seen at every turn, so its second runs from the start)
        let steady = self.folderstorm_characters.memory.pages.observe(now, pages);
        let released = self.scene.textures.folderstorm_released(since) && settled && steady;
        match self
            .folderstorm_characters
            .memory
            .releasing
            .poll(now, self.frame_count, released, keyboard)
        {
            Step::Idle => {}
            Step::Wait { draw: false } => {}
            Step::Wait { draw: true } => self.frame(event_loop),
            Step::Minimise => {
                let waited = self.folderstorm_characters.memory.began.take().map(|at| now.duration_since(at));
                let before = self.folderstorm_characters.memory.before.take().unwrap_or_default();
                let after = self.folderstorm_release_figures().unwrap_or_default();
                let cull = aurora_render::gpu_cull::folderstorm_freed() >> 20;
                log::info!(
                    "session memory: {} after {} ms, the window is minimised: {} MB taken from the driver (was {}), texture pages {} MB (was {}), texture target {} MB, GPU culling {} MB back, probes and shadows {}",
                    if released { "released" } else { "not all released" },
                    waited.unwrap_or_default().as_millis(),
                    after.driver_mb,
                    before.driver_mb,
                    after.pages_mb,
                    before.pages_mb,
                    self.scene.textures.stats.target_bytes >> 20,
                    if self.folderstorm_characters.memory.lowered { cull } else { 0 },
                    if economy_render::lowered_now() {
                        format!(
                            "at their smallest ({} probes, shadow maps of {} px)",
                            economy_render::PROBES,
                            economy_render::SHADOW_RESOLUTION
                        )
                    } else {
                        "as set (the 3D never paused)".to_owned()
                    },
                );
                if let Some(g) = &self.gfx {
                    g.window.set_minimized(true);
                }
                self.folderstorm_characters.memory.minimised_by_host = true;
                self.folderstorm_characters.memory.minimised_at = Some(now);
            }
        }
    }

    /// `memory::PAGES_AFTER_MINIMISE` after Economy's minimise, the pages
    /// and the driver's figure again: what went back once minimised. Not
    /// said if the character left Economy in between.
    fn folderstorm_memory_pages_later(&mut self) {
        let state = &mut self.folderstorm_characters.memory;
        let Some(at) = state.minimised_at else {
            return;
        };
        if at.elapsed() < memory::PAGES_AFTER_MINIMISE {
            return;
        }
        state.minimised_at = None;
        if self.folderstorm_characters.managed.memory().release != Release::Full {
            return;
        }
        if let Some(now) = self.folderstorm_release_figures() {
            log::info!(
                "session memory: {} s after the minimise: texture pages {} MB, {} MB taken from the driver",
                memory::PAGES_AFTER_MINIMISE.as_secs(),
                now.pages_mb,
                now.driver_mb
            );
        }
    }

    /// The window draws at least `memory::RELEASE_FPS` while Economy's
    /// release is under way (`cap`: the frame cap otherwise, 0 for none).
    pub(super) fn folderstorm_release_cap(&self, cap: f32) -> f32 {
        if cap > 0.0 && self.folderstorm_characters.memory.releasing.under_way() {
            cap.max(memory::RELEASE_FPS)
        } else {
            cap
        }
    }

    /// The 3D is paused under Economy's release: the culling buffers go
    /// back to their first size at the next frame, the probes and the
    /// shadow atlas to their smallest (once per release).
    fn folderstorm_memory_lower(&mut self) {
        self.folderstorm_characters.memory.lowered = true;
        aurora_render::gpu_cull::folderstorm_release();
        if economy_render::set(true) {
            self.folderstorm_apply_render_settings();
            log::info!(
                "session memory: the 3D is paused, the GPU culling buffers, the reflection probes ({} slots) and the shadow atlas ({} px maps) go to their smallest; the settings are unchanged",
                economy_render::PROBES,
                economy_render::SHADOW_RESOLUTION
            );
        }
    }

    /// Out of Economy (in front, another standby, a monitor, no host): the
    /// probes and the shadows as the settings say. The culling buffers grow
    /// again by themselves with the first frame that draws the world.
    fn folderstorm_memory_restore(&mut self) {
        self.folderstorm_characters.memory.lowered = false;
        if economy_render::set(false) {
            self.folderstorm_apply_render_settings();
            log::info!("session memory: the reflection probes and the shadows as set again (the probes are captured again)");
        }
    }

    /// The renderer takes the settings as `render_settings` gives them now,
    /// and the probes start over with the slots it has. Without a renderer
    /// (between two, after a loss) the next one is handed them when made.
    fn folderstorm_apply_render_settings(&mut self) {
        if let Some(g) = &mut self.gfx {
            g.renderer.apply_settings(self.settings.render_settings());
        }
        self.probes.reset();
    }

    /// What the driver counts for this process and the texture pages hold
    /// now, in megabytes (None without a renderer or a driver's figure).
    fn folderstorm_release_figures(&self) -> Option<ReleaseFigures> {
        let g = self.gfx.as_ref()?;
        let video = g.renderer.video_memory.read(&g.renderer.device)?;
        Some(ReleaseFigures {
            driver_mb: video.usage >> 20,
            pages_mb: g.renderer.textures.page_bytes() >> 20,
        })
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
