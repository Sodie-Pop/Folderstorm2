//! Folderstorm: Hibernation, a managed viewer's side
//! (`folderstorm/plans/deep-sleep.md`, P2; the rules are
//! `folderstorm_session::sleep`, the host's side `folderstorm_session::memory`).
//!
//! - **Sleep.** The host asks for Hibernation (Economy plus a word);
//!   Economy's release minimises the window (`memory.rs`). Once it has been
//!   minimised for the host's grace, not active and without the keyboard,
//!   the viewer does the GPU-loss recovery's bookkeeping
//!   (`forget_gpu_scene`), lets go of what else holds the device (the
//!   texture streamer's staging pool and uploads, Blender Live, media,
//!   terrain) and drops the whole renderer: device, geometry, textures,
//!   swapchain. The window, egui's state and its CPU copy of the textures,
//!   the world and every cache stay.
//! - **While asleep** the event loop waits [`sleep::TICK`] between turns and
//!   each turn runs the first half of `App::frame` (network, inventory,
//!   chat, the local assistant), which returns where the renderer is taken.
//!   The world is paused, the session goes on. The host is told
//!   `hibernating` and no video memory.
//! - **Wake.** The host makes it active or shows it, a monitor on screen
//!   watches it, the user restores or focuses the window, or the window is
//!   closed (it wakes to ask, then quits): the window comes back first,
//!   then, at a later turn once it has a size, a new renderer on the same
//!   window, the recovery's second half (`adopt_gpu_scene`) and egui's
//!   textures sent again. No "GPU recovered" notice, no loss counted. A
//!   renderer that cannot be made is tried again every
//!   [`sleep::RETRY`]; the active character gives up after
//!   [`sleep::ACTIVE_ATTEMPTS`] (Aurora's failed-recovery path), one
//!   behind stays asleep.

use super::*;
use crate::folderstorm_characters::Asleep;
use ::folderstorm_session::ipc::{Release, Role};
use ::folderstorm_session::memory;
use ::folderstorm_session::sleep::{self, Decision, Facts, Restore, Retry, Wake, WakeFacts};

impl App {
    /// The window without a renderer, while asleep.
    pub(super) fn folderstorm_asleep(&self) -> bool {
        self.folderstorm_characters.sleep.asleep.is_some()
    }

    /// Every turn of the event loop of a managed viewer, after Economy's
    /// pump: asleep, the wake and the paced frames; awake, whether to
    /// hibernate.
    pub(super) fn folderstorm_sleep_pump(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let state = &mut self.folderstorm_characters.sleep;
        if matches!(self.screen, Screen::World) {
            state.in_world_since.get_or_insert(now);
        } else {
            state.in_world_since = None;
        }
        if self.folderstorm_asleep() {
            self.folderstorm_sleep_turn(event_loop, now);
        } else if self.gfx.is_some() {
            self.folderstorm_sleep_check(now);
        }
    }

    /// `window_event` for a window without a renderer: a close while
    /// asleep wakes the window to ask (the exit prompt needs frames), as
    /// its close button does awake. Focus and restore are seen by the next
    /// turn from the window itself.
    pub(in crate::app) fn folderstorm_sleep_event(&mut self, event: &WindowEvent) {
        if self.folderstorm_asleep() && matches!(event, WindowEvent::CloseRequested) {
            log::info!("session memory: the window is closed while hibernating: it wakes to ask");
            self.folderstorm_characters.sleep.closing = true;
            self.request_exit();
        }
    }

    /// The grace this viewer was given, or the default.
    fn folderstorm_hibernate_after(&self) -> Duration {
        let seconds = self.folderstorm_characters.managed.memory().hibernate_after_s;
        let seconds = if seconds == 0 {
            memory::DEFAULT_HIBERNATE_AFTER_S
        } else {
            memory::clamp_hibernate_after(seconds)
        };
        Duration::from_secs(u64::from(seconds))
    }

    /// Awake: Hibernation asked, minimised long enough, nothing keeping it
    /// in Economy: the renderer goes.
    fn folderstorm_sleep_check(&mut self, now: Instant) {
        let Some(g) = &self.gfx else {
            return;
        };
        let minimised = g.window.is_minimized() == Some(true);
        let managed = &self.folderstorm_characters.managed;
        let facts = Facts {
            hibernate: managed.memory().hibernate,
            after: self.folderstorm_hibernate_after(),
            minimised_by_host: self.folderstorm_characters.memory.minimised_by_host,
            minimised,
            keyboard: self.folderstorm_focused(),
            active: managed.role() == Some(Role::Active),
            releasing: self.folderstorm_characters.memory.releasing.under_way(),
            in_world_for: self.folderstorm_characters.sleep.in_world_since.map(|at| now.duration_since(at)),
            voice: self.voice.connected,
            heard: !managed.sound_muted(),
            vr: self.vr_set_up,
            no_egui_copy: !gpu_recovery::shadow_kept(),
            photo: self.snapshots.drawing(),
        };
        let state = &mut self.folderstorm_characters.sleep;
        match state.grace.decide(&facts, now) {
            Decision::Idle | Decision::Grace { .. } => state.stayed = None,
            Decision::Stay(stay) => {
                if state.stayed.replace(stay) != Some(stay) {
                    log::info!("session memory: Hibernation waits, Economy for now: {}", stay.reason());
                }
            }
            Decision::Sleep => self.folderstorm_hibernate(now),
        }
    }

    /// The renderer goes; the window stays, minimised.
    fn folderstorm_hibernate(&mut self, now: Instant) {
        let waited = self.folderstorm_characters.sleep.grace.waited(now);
        let driver_mb = self.folderstorm_release_figures().map_or(0, |f| f.driver_mb);
        let Some(gfx) = self.gfx.take() else {
            return;
        };
        let Gfx {
            mut renderer,
            egui_state,
            window,
            ..
        } = gfx;
        let maximized = window.is_maximized();
        let started = Instant::now();
        // what the scene and the app hold on the renderer, as a GPU loss
        // does (bookkeeping only), then what else holds its device
        self.forget_gpu_scene(&mut renderer);
        self.scene.textures.folderstorm_forget_gpu();
        self.blender_live.gpu_reset(None);
        self.media.gpu_reset();
        for region in self.world.regions.values_mut() {
            region.terrain_surface.gpu_reset();
        }
        drop(egui_state);
        // (joins the render thread; the window's surface goes with it)
        drop(renderer);
        let state = &mut self.folderstorm_characters.sleep;
        state.grace.reset();
        state.stayed = None;
        state.waking = None;
        state.rebuild.reset();
        state.pacer = Default::default();
        state.closing = false;
        state.asleep = Some(Asleep {
            window,
            maximized,
            since: now,
        });
        log::info!(
            "session memory: hibernating after {} s minimised, renderer dropped in {} ms (was {driver_mb} MB taken from the driver)",
            waited.as_secs(),
            started.elapsed().as_millis()
        );
    }

    /// Asleep: a reason to wake, the renderer once the window has a size,
    /// and otherwise the first half of a frame every tick.
    fn folderstorm_sleep_turn(&mut self, event_loop: &ActiveEventLoop, now: Instant) {
        let Some(window) = self.folderstorm_characters.sleep.asleep.as_ref().map(|a| a.window.clone()) else {
            return;
        };
        let size = window.inner_size();
        let ready = sleep::can_build((size.width, size.height), window.is_minimized());
        if self.folderstorm_characters.sleep.waking.is_none() {
            let managed = &self.folderstorm_characters.managed;
            let facts = WakeFacts {
                active: managed.role() == Some(Role::Active),
                watched: managed.memory().release == Release::Lowered,
                show: self.folderstorm_characters.report.show.is_some(),
                closing: std::mem::take(&mut self.folderstorm_characters.sleep.closing),
                focused: window.has_focus(),
                restored: ready,
            };
            if let Some(why) = sleep::wake(&facts) {
                self.folderstorm_wake_begin(&window, why, now);
            }
        }
        if self.folderstorm_characters.sleep.waking.is_some() && ready && self.folderstorm_characters.sleep.rebuild.due(now) {
            // (awake, or closing after the active character gave up)
            if self.folderstorm_wake_build(event_loop, &window, now) {
                return;
            }
        }
        // still asleep: the session goes on, a frame's first half a tick
        if self.folderstorm_characters.sleep.pacer.due(now) {
            self.frame(event_loop);
        }
        let next = self.folderstorm_characters.sleep.pacer.next(Instant::now());
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(next));
    }

    /// The window comes back as the reason says; the renderer follows at a
    /// later turn, once the window has a size.
    fn folderstorm_wake_begin(&mut self, window: &Window, why: Wake, now: Instant) {
        log::info!("session memory: waking from Hibernation: {}", why.reason());
        let state = &mut self.folderstorm_characters.sleep;
        state.waking = Some((why, now));
        state.rebuild.reset();
        match why.restores() {
            Restore::Front => match self.folderstorm_characters.report.show.take() {
                // (on the stage, as the host shows a window)
                Some(stage) => self.folderstorm_show(stage),
                None => {
                    window.set_minimized(false);
                    crate::folderstorm_characters::foreground::bring_forward(window);
                }
            },
            Restore::Behind => crate::folderstorm_characters::foreground::restore_behind(window),
            Restore::Nothing => {}
        }
    }

    /// A new renderer on the same window, the scene adopted on it. True
    /// once awake, or when the active character gave up and the viewer
    /// closes as after a failed GPU recovery (nothing more this turn);
    /// false when it could not be made yet (tried again, or asleep again
    /// behind).
    fn folderstorm_wake_build(&mut self, event_loop: &ActiveEventLoop, window: &Arc<Window>, now: Instant) -> bool {
        let started = Instant::now();
        let mut renderer = match self.open_renderer(window) {
            Ok(renderer) => renderer,
            Err(e) => {
                let why = self.folderstorm_characters.sleep.waking.map(|(why, _)| why);
                let active = self.folderstorm_characters.managed.role() == Some(Role::Active) || why == Some(Wake::Closing);
                let state = &mut self.folderstorm_characters.sleep;
                match state.rebuild.failed(now, active) {
                    Retry::Again => log::warn!(
                        "session memory: no renderer to wake with ({e}), tried again in {} s",
                        sleep::RETRY.as_secs()
                    ),
                    Retry::StayAsleep => {
                        log::warn!("session memory: no renderer to wake with ({e}): it stays asleep behind");
                        state.waking = None;
                    }
                    Retry::GiveUp => {
                        log::error!(
                            "session memory: no renderer to wake with after {} attempts ({e}): closing, as after a failed GPU recovery",
                            state.rebuild.failures()
                        );
                        crate::crash::gpu_lost();
                        self.close_while_gpu_stalled(event_loop);
                        return true;
                    }
                }
                return false;
            }
        };
        renderer.apply_settings(self.settings.render_settings());
        self.adopt_gpu_scene(&mut renderer);
        self.note_gpus(&renderer);
        self.screen_rate.poll(window, Instant::now());
        let egui_state = self.egui_state_for(window, &renderer);
        let state = &mut self.folderstorm_characters.sleep;
        let Some(asleep) = state.asleep.take() else {
            return false;
        };
        state.waking = None;
        state.rebuild.reset();
        state.pacer = Default::default();
        state.grace.reset();
        self.gfx = Some(Gfx {
            renderer,
            egui_state,
            window: asleep.window,
            // (it was shown before it slept)
            shown: true,
            maximize_on_show: asleep.maximized,
        });
        // the new egui renderer has none of egui's textures
        self.egui_replay = true;
        // a new renderer, not a recovery: nothing to count or say
        self.gpu_stall = Default::default();
        self.folderstorm_characters.loss_waiting = false;
        self.folderstorm_characters.stall_held = false;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        log::info!(
            "session memory: awake after {} s hibernating: renderer {} ms",
            now.duration_since(asleep.since).as_secs(),
            started.elapsed().as_millis()
        );
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
        true
    }
}
