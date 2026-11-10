//! Folderstorm: the frame's side of several characters on one PC
//! (`crate::folderstorm_characters`, session-host plan sections 6.1 to
//! 6.4): the 3D policy while another Folderstorm2 window is in front, its
//! notice, a profile's raise request, and the GPU rules that keep a window
//! behind from showing a box over the active character, lowering its
//! graphics or closing (R4, R5). The decisions themselves are the
//! `folderstorm-session` crate's pure functions.

use super::*;
use ::folderstorm_session::behind::{self, Behind3d};
use ::folderstorm_session::gpu::{self, Loss};

impl App {
    /// The policy in force: the profile's, or the demo's for the run.
    fn folderstorm_policy(&self) -> Behind3d {
        match crate::folderstorm_characters::demo_behind() {
            Some(Some(policy)) => policy,
            _ => self.settings.folderstorm_characters.behind_3d,
        }
    }

    /// `AURORA_DEMO_SESSION_BEHIND`: another Folderstorm2 window counts as
    /// in front, until a click in this one.
    fn folderstorm_demo_behind(&self) -> bool {
        self.demo && crate::folderstorm_characters::demo_behind().is_some() && !self.folderstorm_characters.demo_released
    }

    /// Every frame, after the frame cap: whether the window is behind
    /// another Folderstorm2 window, the 3D pause of the Off policy (read by
    /// `drawing_paused` from the next frame), the window's place for the GPU
    /// rules, and the cap with the policy's rate. `timed`: a capture or a
    /// profile run, `vr`: a headset in use, `minimized`: the window is.
    pub(super) fn folderstorm_characters_frame(&mut self, cap: f32, timed: bool, vr: bool, minimized: bool) -> f32 {
        if self.folderstorm_demo_behind() && self.egui_ctx.input(|i| i.pointer.any_pressed()) {
            log::info!("session demo: clicked, no other window in front any more");
            self.folderstorm_characters.demo_released = true;
        }
        let demo = self.folderstorm_demo_behind();
        let focused = self.focused && !demo;
        let behind = behind::is_behind(focused, demo || self.instances.peers().in_front());
        let policy = self.folderstorm_policy();
        // (the demo pauses a capture too, as the busy pause's demo does)
        let exempt = timed && !demo;
        let paused = behind::paused(policy, behind, exempt || vr || self.snapshots.drawing());
        let state = &mut self.folderstorm_characters;
        if paused != state.paused {
            if paused {
                log::info!("3D paused: another Folderstorm2 window is in front (policy Off)");
            } else {
                log::info!("3D resumed");
            }
        }
        state.behind = behind;
        state.paused = paused;
        state.place = gpu::place(self.focused, minimized, self.occluded, demo);
        behind::cap(cap, policy, behind, self.settings.folderstorm_characters.behind_fps, exempt)
    }

    /// The notice over the world paused by the Off policy (the busy card's
    /// own notice takes its place when both apply).
    pub(super) fn folderstorm_characters_notice(&self, ctx: &egui::Context, p: &crate::theme::Palette) {
        if !self.folderstorm_characters.paused || self.busy_pause.paused() || self.last_render.video_memory_full || !self.in_world() {
            return;
        }
        // as the busy card's pause shows it (busy_pause.rs): the paused
        // world blurred and darkened under the windows, the notice over it
        let id = egui::Id::unique("folderstorm-behind-notice");
        let layer = egui::LayerId::new(egui::Order::Background, id);
        aurora_render::blurred_backdrop(ctx, layer);
        ctx.layer_painter(layer)
            .rect_filled(ctx.content_rect(), 0.0, p.navy.gamma_multiply(0.55));
        let room = crate::ui::layout::work_area(ctx);
        egui::Area::new(id)
            .order(egui::Order::Background)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(room.center())
            .interactable(false)
            .show(ctx, |ui| {
                crate::ui::widgets::overlay(ui, p, egui::Margin::symmetric(16, 10), |ui| {
                    let text = aurora_i18n::tr!("folderstorm-behind-paused-notice", product = folderstorm_brand::PRODUCT);
                    ui.label(egui::RichText::new(text).color(p.amber));
                });
            });
    }

    /// A link handed over by a later launch: true when it is a second
    /// launch of this profile asking for this window, which comes forward.
    pub(super) fn folderstorm_raise_request(&self, url: &str) -> bool {
        if !crate::folderstorm_characters::profile::is_raise_request(url) {
            return false;
        }
        log::info!("profile: launched again, the window comes forward");
        if let Some(g) = &self.gfx {
            g.window.set_minimized(false);
            g.window.focus_window();
        }
        true
    }

    /// The GPU device is lost (`gpu_stall_frame`, before Aurora's own
    /// handling): true when this module took it. Not in front, the loss
    /// waits (no box, no count, no safe level saved, no close) while the
    /// session goes on; back in front, it recovers without counting.
    /// `recovers`: the recovery is on and no headset is in use.
    pub(super) fn folderstorm_gpu_loss(&mut self, recovers: bool) -> bool {
        let state = &mut self.folderstorm_characters;
        match gpu::on_loss(state.place, state.loss_waiting, recovers) {
            Loss::Wait => {
                if !std::mem::replace(&mut state.loss_waiting, true) {
                    log::error!(
                        "the graphics card is lost while the window is not in front ({:?}): the session goes on, the window recovers when it comes to the front",
                        state.place
                    );
                    crate::crash::gpu_loss_snapshot();
                }
                true
            }
            Loss::Recover => {
                state.loss_waiting = false;
                log::error!("the graphics card was lost behind: recovering on a new window and device, not counted");
                self.gpu_stall.recover = Some(false);
                true
            }
            Loss::Upstream => {
                state.loss_waiting = false;
                false
            }
        }
    }

    /// Whether the "the card does not answer" box may show now: in front
    /// only (`stalled`: how long the card has held the frame).
    pub(super) fn folderstorm_gpu_notice_allowed(&mut self, stalled: Option<Duration>) -> bool {
        let state = &mut self.folderstorm_characters;
        let allowed = gpu::stall_notice_allowed(state.place);
        let held = !allowed && stalled.is_some();
        if held && !state.stall_held {
            log::warn!(
                "the graphics card does not answer while the window is not in front ({:?}): its box waits for the front",
                state.place
            );
        }
        state.stall_held = held;
        allowed
    }
}
