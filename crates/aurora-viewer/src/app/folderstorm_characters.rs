//! Folderstorm: the frame's side of several characters on one PC
//! (`crate::folderstorm_characters`, session-host plan sections 6.1 to
//! 6.4): the 3D policy while another Folderstorm2 window is in front, its
//! notice, a profile's raise request, and the GPU rules that keep a window
//! behind from showing a box over the active character, lowering its
//! graphics or closing (R4, R5). The decisions themselves are the
//! `folderstorm-session` crate's pure functions.

use super::*;
use crate::folderstorm_characters::managed::Event as HostEvent;
use ::folderstorm_session::behind::{self, Behind3d};
use ::folderstorm_session::gpu::{self, Loss};
use ::folderstorm_session::ipc;

/// A managed viewer looks at its state for the host this often, and tells
/// it when it changed, or after [`REPORT_EVERY`] anyway.
const REPORT_CHECK: Duration = Duration::from_millis(250);
const REPORT_EVERY: Duration = Duration::from_secs(1);

impl App {
    /// The policy in force: the profile's, or the demo's for the run.
    fn folderstorm_policy(&self) -> Behind3d {
        match crate::folderstorm_characters::demo_behind() {
            Some(Some(policy)) => policy,
            _ => self.settings.folderstorm_characters.behind_3d,
        }
    }

    /// The window has the keyboard: it exists and has the focus
    /// (`gpu::has_keyboard`). Never `self.focused` alone, which starts true
    /// before any focus event: a character started behind would count as
    /// in front and claim the focus in its first report.
    ///
    /// A frame takes the window out of `self.gfx` while it is drawn, and
    /// most of these rules are asked during the frame: the window's answer
    /// is remembered (`window_focus`), or every window would count as
    /// without the keyboard there (the background cap in front, a loss in
    /// front left to wait).
    fn folderstorm_focused(&self) -> bool {
        let window = self.gfx.as_ref().map(|g| g.window.has_focus());
        let window = crate::folderstorm_characters::window_focus(&self.folderstorm_characters.window_focus, window);
        gpu::has_keyboard(self.focused, window)
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
        let has_keyboard = self.folderstorm_focused();
        let focused = has_keyboard && !demo;
        // managed, the session host says who is behind and with which
        // policy; otherwise the instances registry and this profile do
        let (behind, policy) = match self.folderstorm_characters.managed.behind(has_keyboard) {
            Some(managed) if !demo => managed,
            _ => (
                behind::is_behind(focused, demo || self.instances.peers().in_front()),
                self.folderstorm_policy(),
            ),
        };
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
        state.place = gpu::place(has_keyboard, minimized, self.occluded, demo);
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

    /// In front, for the rules that used the keyboard focus (frame cap,
    /// texture release, busy pause, the instances registry): focused, or
    /// the session host's active character while the user is in the host's
    /// window (rule R3).
    pub(super) fn folderstorm_in_front(&self) -> bool {
        self.folderstorm_characters.managed.in_front(self.folderstorm_focused())
    }

    /// `sync_minimized_mute`'s update of the transient mute: Aurora's
    /// "mute when minimised", or a background character of the session
    /// host. True when the mute changed. Nothing saved is touched.
    pub(super) fn folderstorm_session_mute(&mut self, minimized: Option<bool>) -> bool {
        let was = self.minimized_mute.active;
        self.minimized_mute.update(self.settings.audio.mute_when_minimized, minimized);
        self.minimized_mute.active |= self.folderstorm_characters.managed.sound_muted();
        self.minimized_mute.active != was
    }

    /// `shutdown`, with the graphics card holding a frame: false when the
    /// session host asked this viewer to quit. Its Quit is no close from
    /// the card's box: the session is not reported as ended in GPU trouble
    /// and the next start keeps its graphics (rules R4, R5: a character
    /// behind never lowers them).
    pub(super) fn folderstorm_gpu_close_counts(&self, stalled: Duration) -> bool {
        if !gpu::stalled_close_reported(self.folderstorm_characters.quit_by_host) {
            log::warn!(
                "quitting at the session host's request while the graphics card holds a frame ({} s): not counted as a close in GPU trouble",
                stalled.as_secs()
            );
            return false;
        }
        true
    }

    /// Every turn of the event loop, in a viewer the session host started:
    /// the host's commands are carried out and the host is told how this
    /// viewer does. Never waits. Not the frame's job: frames may be rare
    /// while the window is minimised (Economy), commands must not be.
    pub(super) fn folderstorm_managed_pump(&mut self) {
        let Some(link) = crate::folderstorm_characters::managed::link() else {
            return;
        };
        link.turn();
        let events: Vec<HostEvent> = link.poll().collect();
        for event in events {
            match event {
                HostEvent::Linked { slot } => {
                    log::info!("session host: managed, character {}", slot + 1);
                    self.folderstorm_characters.managed.link();
                }
                HostEvent::Command(ipc::ToViewer::Assign(assignment)) => {
                    let revokes_voice = self.folderstorm_characters.managed.revokes_voice(&assignment);
                    if self.folderstorm_characters.managed.assign(assignment) {
                        if revokes_voice {
                            // off now, not at the next frame (rare while
                            // minimised): the session is left and the
                            // microphone closed, as `update_voice` does
                            // with voice not enabled; the host hands the
                            // voice over once the status says it is off
                            log::info!("session host: voice taken away, leaving the voice channel now");
                            self.voice.restart();
                        }
                        log::info!(
                            "session host: {:?}, {:?} standby, sound {}, voice {}",
                            assignment.role,
                            assignment.standby,
                            if assignment.sound_muted { "cut" } else { "on" },
                            if assignment.voice_allowed { "allowed" } else { "off" }
                        );
                        self.sync_minimized_mute();
                    }
                }
                HostEvent::Command(ipc::ToViewer::Show { stage }) => self.folderstorm_characters.report.show = Some(stage),
                HostEvent::Command(ipc::ToViewer::Minimise) => {
                    // (never the active character, never a window with the keyboard)
                    if !self.folderstorm_in_front()
                        && let Some(g) = &self.gfx
                    {
                        g.window.set_minimized(true);
                    }
                }
                HostEvent::Command(ipc::ToViewer::Quit) => {
                    log::info!("session host: asked to quit");
                    self.folderstorm_characters.quit_by_host = true;
                    self.request_exit();
                    self.confirm_exit(ui::exit::Action::Quit);
                }
                HostEvent::Gone => {
                    if self.folderstorm_characters.managed.linked() {
                        log::info!("session host: gone, this viewer goes on by itself");
                    }
                    self.folderstorm_characters.managed.release();
                    self.sync_minimized_mute();
                }
            }
        }
        if !self.folderstorm_characters.managed.linked() {
            self.folderstorm_characters.report.show = None;
            return;
        }
        // (a window not shown yet would be revealed early, or lose the
        // placing to its own first show: it waits)
        if self.gfx.as_ref().is_some_and(|g| g.shown)
            && let Some(stage) = self.folderstorm_characters.report.show.take()
        {
            self.folderstorm_show(stage);
        }
        let now = Instant::now();
        let report = &self.folderstorm_characters.report;
        if report.checked.is_some_and(|at| now.duration_since(at) < REPORT_CHECK) {
            return;
        }
        let status = self.folderstorm_status();
        let report = &mut self.folderstorm_characters.report;
        report.checked = Some(now);
        let due = report
            .sent
            .as_ref()
            .is_none_or(|(sent, at)| *sent != status || now.duration_since(*at) >= REPORT_EVERY);
        if due && link.report(status.clone()) {
            report.sent = Some((status, now));
        }
    }

    /// The host brings this character forward: restored, on the stage when
    /// there is one, then focused.
    fn folderstorm_show(&mut self, stage: Option<ipc::Stage>) {
        let Some(g) = &self.gfx else {
            return;
        };
        let window = &g.window;
        window.set_minimized(false);
        // (a full-screen window, a capture and a forced size keep their place)
        let placed = window.fullscreen().is_none() && self.capture.is_none() && forced_window_size().is_none();
        if let Some(stage) = stage.filter(|s| placed && s.valid() && on_a_monitor(window, s)) {
            use winit::dpi::{PhysicalPosition, PhysicalSize};
            if stage.maximised {
                if !window.is_maximized() {
                    // onto the stage's screen first: a window maximises
                    // where it is
                    let (x, y) = (stage.x + (stage.width / 4) as i32, stage.y + (stage.height / 4) as i32);
                    window.set_outer_position(PhysicalPosition::new(x, y));
                    window.set_maximized(true);
                }
            } else {
                if window.is_maximized() {
                    window.set_maximized(false);
                }
                // the stage is an outer rectangle: the frame's share off
                let (outer, inner) = (window.outer_size(), window.inner_size());
                let width = stage.width.saturating_sub(outer.width.saturating_sub(inner.width)).max(1);
                let height = stage.height.saturating_sub(outer.height.saturating_sub(inner.height)).max(1);
                window.set_outer_position(PhysicalPosition::new(stage.x, stage.y));
                let _ = window.request_inner_size(PhysicalSize::new(width, height));
            }
            self.geom_changed = Some(Instant::now());
        }
        window.focus_window();
    }

    /// What the host is told of this viewer.
    fn folderstorm_status(&mut self) -> ipc::Status {
        let logged_in = self.in_world();
        let report = &mut self.folderstorm_characters.report;
        if std::mem::replace(&mut report.logged_in, logged_in) != logged_in {
            report.generation = report.generation.wrapping_add(1);
        }
        let state = &self.folderstorm_characters;
        let (screen, window, gpu) = match &self.gfx {
            None => (ipc::Screen::Starting, ipc::WindowState::default(), ipc::Gpu::Ok),
            Some(g) => {
                let screen = match self.screen {
                    Screen::Login => ipc::Screen::Login,
                    Screen::Loading { .. } => ipc::Screen::Loading,
                    Screen::World => ipc::Screen::World,
                };
                let minimised = g.window.is_minimized().unwrap_or(false);
                let rect = (!minimised).then(|| g.window.outer_position().ok()).flatten().map(|at| {
                    let size = g.window.outer_size();
                    ipc::Stage {
                        x: at.x,
                        y: at.y,
                        width: size.width,
                        height: size.height,
                        maximised: g.window.is_maximized(),
                    }
                });
                let window = ipc::WindowState {
                    rect,
                    minimised,
                    fullscreen: g.window.fullscreen().is_some(),
                };
                let gpu = if g.renderer.device_lost() {
                    ipc::Gpu::Lost
                } else if g.renderer.stalled_for().is_some() {
                    ipc::Gpu::Stalled
                } else if self.busy_pause.paused() {
                    ipc::Gpu::Busy
                } else {
                    ipc::Gpu::Ok
                };
                (screen, window, gpu)
            }
        };
        ipc::Status {
            generation: state.report.generation,
            assign_seq: state.managed.seq(),
            screen,
            name: if logged_in { self.world.own_name() } else { String::new() },
            agent: if logged_in {
                self.world.agent_id.to_string()
            } else {
                String::new()
            },
            grid: if logged_in {
                format!("{:?}", self.settings.grid)
            } else {
                String::new()
            },
            region: if logged_in { self.world.region_name() } else { String::new() },
            unread: if logged_in {
                u32::try_from(self.world.conversations_badge()).unwrap_or(u32::MAX)
            } else {
                0
            },
            // (no window yet: never focused)
            focused: self.folderstorm_focused(),
            window,
            gpu,
            paused_3d: state.paused,
            sound_muted: self.minimized_mute.active,
            // given up only once the voice is off too
            voice_allowed: state.managed.voice_report(self.voice.connected),
        }
        .cleaned()
    }
}

/// Whether the stage shows on one of the screens: a place saved when
/// another screen was plugged in is not taken.
fn on_a_monitor(window: &winit::window::Window, stage: &ipc::Stage) -> bool {
    const SEEN: i64 = 120;
    window.available_monitors().any(|monitor| {
        let (at, size) = (monitor.position(), monitor.size());
        let overlap = |a: i64, a_len: i64, b: i64, b_len: i64| (a + a_len).min(b + b_len) - a.max(b);
        overlap(i64::from(stage.x), i64::from(stage.width), i64::from(at.x), i64::from(size.width)) >= SEEN
            && overlap(i64::from(stage.y), i64::from(stage.height), i64::from(at.y), i64::from(size.height)) >= SEEN
    })
}
