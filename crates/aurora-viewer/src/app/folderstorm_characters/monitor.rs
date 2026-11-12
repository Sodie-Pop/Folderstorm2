//! Folderstorm: a managed viewer's monitor (session-host plan section 12,
//! checkpoint 5). While the session host shows this background
//! character's monitor, the viewer takes a small picture of its own view
//! now and then and hands it to its link; between two pictures its 3D
//! stays off, as for any background character.
//!
//! A picture is one frame of Aurora's own photo path
//! (`Renderer::snapshot_request`, app/snapshot.rs): drawn offscreen at the
//! picture's size with the frame's own parameters (so with every RLVa
//! effect the renderer draws on screen: `rlva_spheres`, `rlva_overlays`,
//! `rlva_resolution_divisor`, restricted textures), without the interface
//! or the HUDs, then read back. Nothing new is drawn for it and nothing is
//! presented: the window keeps the picture it shows.
//!
//! - **When** is the `folderstorm-session` crate's `monitor::Schedule`:
//!   one picture at a time, at the host's rate, slower when the card is
//!   slow to answer, a rare one while Aurora's busy pause is on, none
//!   while the card holds a frame, is lost or is out of memory, the window
//!   is minimised, the character is not in the world, or a photo of the
//!   user's is being drawn.
//! - **Never a long wait on the card.** The frame that carries a picture
//!   waits [`PICTURE_WAIT`] for it instead of the photos' ten seconds
//!   (`Renderer::snapshot_wait`); a picture not back by then comes with a
//!   later frame (the renderer's late answer) and one not back after
//!   `monitor::PICTURE_TIMEOUT` is given up (`Renderer::drop_snapshot`),
//!   which lowers the rate.
//! - **Nothing is kept.** The pixels go to the link's place for one
//!   picture and are gone from here; no file, no texture, no history.
//!
//! Two calls from `App::frame`, both marked `// Folderstorm:`: before the
//! frame is laid out (next to `photo_frame`) and after it is drawn (before
//! `photo_drawn`, which would drop an answer no photo waits for).

use super::*;
use crate::app::snapshot::PhotoFrame;
use crate::folderstorm_characters::managed::{self, Picture};
use ::folderstorm_session::monitor::{self, Conditions};
use aurora_render::{Renderer, SnapshotSpec};

/// How long the frame that carries a picture waits for the card: about
/// two frames at 30 a second. Past it the main thread goes on and the
/// picture comes with a later frame.
const PICTURE_WAIT: Duration = Duration::from_millis(60);

impl App {
    /// What the schedule needs to know of this frame. `photo`: a photo of
    /// the user's is laid out this frame.
    fn folderstorm_monitor_conditions(&self, renderer: &Renderer, photo: bool, minimised: bool) -> Conditions {
        let awaited = self.folderstorm_characters.monitor.asked.is_some();
        Conditions {
            in_world: self.in_world(),
            minimised,
            // (a picture given up whose answer is still on the card: nothing
            // more is asked until the card has answered that one)
            card_gone: renderer.device_lost()
                || renderer.failed()
                || renderer.stalled_for().is_some()
                || self.scene.textures.drawing_paused()
                || self.last_render.video_memory_full
                || (!awaited && renderer.snapshot_pending()),
            card_busy: self.busy_pause.paused(),
            occupied: photo
                || self.snapshots.drawing()
                || self.capture.is_some()
                || self.frame_profile.enabled()
                || renderer.vr_engaged()
                || (!awaited && renderer.snapshot_request.is_some()),
        }
    }

    /// Before a frame is laid out, after `photo_frame`: the monitor's
    /// picture this frame carries, if one is due. `photo`: the user's own
    /// photo frame, which always comes first and is returned as it is.
    pub(in crate::app) fn folderstorm_monitor_frame(
        &mut self,
        photo: Option<PhotoFrame>,
        renderer: &mut Renderer,
        minimised: bool,
    ) -> Option<PhotoFrame> {
        let Some(link) = managed::link() else {
            return photo;
        };
        let now = Instant::now();
        let order = self.folderstorm_characters.managed.monitor(self.folderstorm_focused());
        let conditions = self.folderstorm_monitor_conditions(renderer, photo.is_some(), minimised);
        let state = &mut self.folderstorm_characters.monitor;
        if std::mem::replace(&mut state.ordered, order.is_some()) != order.is_some() {
            match order {
                Some(order) => log::info!(
                    "session monitor: on, pictures within {:?} every {} ms",
                    order.size.dims(),
                    order.interval_ms
                ),
                None => {
                    log::info!(
                        "session monitor: off, the 3D stays off ({} pictures sent, {:.0} ms a picture)",
                        state.sent,
                        state.waited.as_secs_f64() * 1000.0 / state.sent.max(1) as f64
                    );
                    (state.sent, state.waited) = (0, Duration::ZERO);
                    // one not written yet is not sent
                    link.drop_picture();
                }
            }
        }
        if let Some(size) = state.asked
            && !renderer.snapshot_pending()
        {
            // asked, and neither answered nor on the card: the frame that
            // carried it never reached the renderer. It is not drawn by a
            // later frame, which is not laid out for it.
            cancel(state, renderer, size);
        }
        if state.asked.is_some() && state.schedule.overdue(now) {
            // the card has had it for too long: given up, its answer is
            // not kept, and the rate drops
            log::warn!(
                "session monitor: a picture waited {:?} for the graphics card, given up; fewer pictures are asked",
                monitor::PICTURE_TIMEOUT
            );
            renderer.drop_snapshot();
            renderer.snapshot_wait = None;
            state.asked = None;
            state.schedule.timed_out();
        }
        let told = state.schedule.state(order, conditions);
        if std::mem::replace(&mut state.state, told) != told && order.is_some() {
            log::info!("session monitor: {told:?}");
        }
        // (a picture awaited is on the card: this frame is an ordinary one)
        if photo.is_some() || state.asked.is_some() || !state.schedule.poll(order, conditions, now) {
            return photo;
        }
        let size = order.and_then(|order| monitor::fit(order.size, renderer.world_size()));
        let Some((width, height)) = size else {
            state.schedule.forget();
            return None;
        };
        let spec = SnapshotSpec {
            width,
            height,
            interface: false,
            huds: false,
            photo: (width, height),
        };
        renderer.snapshot_request = Some(spec);
        renderer.snapshot_wait = Some(PICTURE_WAIT);
        state.asked = Some((width, height));
        // as a photo's frame: the scene is drawn although the 3D is paused
        // behind (`App::scene_paused`), laid out for the picture's size
        self.snapshots.capturing = Some(spec);
        Some(PhotoFrame {
            aspect: width as f32 / height as f32,
            tile: glam::Mat4::IDENTITY,
            height,
            scale: crate::snapshot::viewfinder_scale((width, height), renderer.size(), self.settings.render_scale_percent),
            look: None,
            context: aurora_render::PhotoContext::None,
        })
    }

    /// After a frame was drawn, before `photo_drawn`: the monitor's
    /// picture it carried, or one that comes late, goes to the link.
    pub(in crate::app) fn folderstorm_monitor_drawn(&mut self, renderer: &mut Renderer) {
        let focused = self.folderstorm_focused();
        let generation = self.folderstorm_characters.report.generation;
        let wanted = self.folderstorm_characters.managed.monitor(focused).is_some();
        let state = &mut self.folderstorm_characters.monitor;
        // (not ours: a photo's answer is the photo's)
        let Some(size) = state.asked else {
            return;
        };
        let Some(answer) = renderer.snapshot.take() else {
            if !renderer.snapshot_pending() {
                // neither answered nor on the card: the frame never left
                // (the render thread is gone). Nothing is awaited.
                cancel(state, renderer, size);
            }
            return;
        };
        renderer.snapshot_wait = None;
        state.asked = None;
        // a device lost under the pass reads back nothing that is the view
        let picture = answer.filter(|(w, h, _)| (*w, *h) == size && !renderer.device_lost());
        let Some((width, height, rgba)) = picture else {
            log::warn!("session monitor: a picture was not drawn");
            state.schedule.failed();
            return;
        };
        let waited = state.schedule.waited(Instant::now());
        state.schedule.answered(Instant::now());
        // the monitor closed, or the user came to this window, meanwhile
        let link = managed::link().filter(|_| wanted);
        if let Some(link) = link {
            state.seq += 1;
            state.sent += 1;
            state.waited += waited;
            link.picture(Picture {
                generation,
                seq: state.seq,
                width,
                height,
                rgba,
            });
        }
    }
}

/// The picture asked was never sent to the card: it is not asked any
/// more, and counts as one that failed.
fn cancel(state: &mut crate::folderstorm_characters::Monitoring, renderer: &mut Renderer, size: (u32, u32)) {
    if renderer.snapshot_request.is_some_and(|s| (s.width, s.height) == size) {
        renderer.snapshot_request = None;
    }
    renderer.snapshot_wait = None;
    state.asked = None;
    state.schedule.failed();
}
