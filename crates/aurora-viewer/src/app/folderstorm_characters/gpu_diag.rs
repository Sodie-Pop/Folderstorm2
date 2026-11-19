//! Folderstorm: the frame's side of the GPU loss diagnostics
//! (folderstorm/gpu_diag). Two calls from Aurora's files, both marked
//! `// Folderstorm:`: [`App::folderstorm_gpu_diag_frame`] after
//! `gpu_stall_frame` in `App::frame` (a sample every 5 s, the entry at the
//! loss), and [`App::folderstorm_gpu_recovered`] in
//! `recover_from_gpu_loss` (the outcome, and the notice's text).

use super::*;
use crate::folderstorm::gpu_diag::{self, Context, Entry, Outcome, Peer, Place, Sample, Work};

/// The samples the reason goes by: the last seconds before the loss.
const RECENT: Duration = Duration::from_secs(10);

impl App {
    /// Every frame, after `gpu_stall_frame` (which may have seen the loss
    /// and chosen to wait or recover): a sample of the memory picture every
    /// few seconds while the card works; at the loss, once per renderer,
    /// the entry in the log and in its file.
    pub(in crate::app) fn folderstorm_gpu_diag_frame(&mut self, renderer: &Renderer, window: &Window) {
        if self.folderstorm_characters.gpu_diag.loss.is_some() {
            return;
        }
        let now = Instant::now();
        let lost = renderer.device_lost();
        if !lost && !self.folderstorm_characters.gpu_diag.history.due(now) {
            return;
        }
        let sample = self.folderstorm_gpu_sample(renderer, window);
        self.folderstorm_characters.gpu_diag.history.push(now, sample);
        if !lost {
            return;
        }
        let managed = &self.folderstorm_characters.managed;
        let context = Context {
            driver: aurora_render::loss_details(),
            work: aurora_render::loss_verdict().and_then(Work::from_verdict),
            role: managed.role().map(|role| match role {
                ipc::Role::Active => "active",
                ipc::Role::Background => "background",
                ipc::Role::Monitor(_) => "monitor",
            }),
            standby: managed.standby().map(|standby| match standby {
                ipc::Standby::Warm => "warm",
                ipc::Standby::Economy if managed.memory().hibernate => "hibernation",
                ipc::Standby::Economy => "economy",
            }),
            in_front: self.folderstorm_in_front(),
        };
        let history = self.folderstorm_characters.gpu_diag.history.aged(now);
        let recent: Vec<Sample> = history.iter().filter(|(age, _)| *age <= RECENT).map(|(_, s)| s.clone()).collect();
        let reason = gpu_diag::demo_reason(self.demo).unwrap_or_else(|| gpu_diag::reason(&context, &recent));
        let entry = Entry {
            when: chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            context,
            reason,
            history,
        };
        log::error!("{}", entry.log_line());
        let outcome = if self.folderstorm_characters.loss_waiting {
            Outcome::Waiting
        } else {
            Outcome::Pending
        };
        gpu_diag::write_entry(&entry, outcome);
        log::info!("GPU loss: written to {}", gpu_diag::file_path().display());
        self.folderstorm_characters.gpu_diag.loss = Some(reason);
    }

    /// The memory picture now. No GPU call: the driver's budget is the
    /// probe's latest reading, the card's use Windows' latest counter.
    fn folderstorm_gpu_sample(&self, renderer: &Renderer, window: &Window) -> Sample {
        let video = renderer.video_memory.read(&renderer.device);
        let textures = &self.scene.textures;
        let size = window.inner_size();
        Sample {
            vram_used: video.map(|v| gpu_diag::mb(v.usage)),
            vram_budget: video.map(|v| gpu_diag::mb(v.budget)),
            allocated: video.map(|v| gpu_diag::mb(v.allocated)),
            textures: gpu_diag::mb(renderer.textures.bytes()),
            texture_pages: gpu_diag::mb(renderer.textures.page_bytes()),
            texture_target: gpu_diag::mb(textures.stats.target_bytes),
            pressure: textures.stats.pressure,
            pressure_max: crate::scene::texture_budget::MAX_PRESSURE,
            card_used: textures.card_used().map(gpu_diag::mb),
            card_total: renderer.info.dedicated_mb,
            peers: self
                .instances
                .peers()
                .list
                .iter()
                .map(|p| Peer {
                    used_mb: gpu_diag::mb(p.usage),
                    in_front: p.active,
                })
                .collect(),
            window: (size.width, size.height),
            place: match self.folderstorm_characters.place {
                gpu::Place::Front => Place::Front,
                gpu::Place::Behind => Place::Behind,
                gpu::Place::Minimised => Place::Minimised,
            },
        }
    }

    /// The recovery made its new window and renderer in `ms` (`safe`: with
    /// the safe graphics level): the entry's outcome, and the notice's text,
    /// the likely reason first, else Aurora's own text (`aurora`).
    pub(in crate::app) fn folderstorm_gpu_recovered(&mut self, ms: u128, safe: bool, aurora: &str) -> String {
        let diag = &mut self.folderstorm_characters.gpu_diag;
        // the new renderer's history starts afresh
        diag.history.clear();
        let reason = diag.loss.take();
        if reason.is_some() {
            gpu_diag::write_outcome(Outcome::Recovered { ms, safe });
        }
        let managed = self.folderstorm_characters.managed.linked();
        gpu_diag::notice(reason.unwrap_or(gpu_diag::Reason::Unknown), managed, safe, aurora)
    }
}
