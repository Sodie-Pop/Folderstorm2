//! Folderstorm: the top bar's media button and its Nearby media panel in
//! the app (the rules are in `crate::folderstorm::nearby_media`). The panel's
//! actions run on Aurora's own media manager and radio, as the Legacy
//! Viewer's LLPanelNearByMedia does on LLViewerMedia: a media source is
//! started through the media filter as the parcel media button and a click
//! on a face do, and the music as the radio button does.

use super::*;
use crate::folderstorm::nearby_media::{self as nearby, Action, MediaFacts, MusicFacts, Op, Snapshot};

impl App {
    /// The rows of the Nearby media panel as they are now.
    fn nearby_media_snapshot(&self) -> Snapshot {
        nearby::snapshot(
            self.media.impls.iter().map(MediaFacts::of),
            self.world.parcel.as_deref().map(|pc| pc.media_url.as_str()),
            MusicFacts {
                station: self.parcel_music.station(),
                requested: self.parcel_music.requested(),
                status: self.music_status,
            },
            &self.settings.media,
            self.media.plugins_found().is_none(),
        )
    }

    /// Before the top bar is drawn: the panel's rows while it is open.
    pub(super) fn folderstorm_nearby_media_publish(&self, ctx: &egui::Context) {
        if nearby::wants_snapshot(ctx) {
            nearby::publish(ctx, self.nearby_media_snapshot());
        }
    }

    /// The top bar's media button, or one of the panel's actions.
    pub(super) fn folderstorm_media_action(&mut self, action: Action) {
        let rows = self.nearby_media_snapshot().rows;
        for op in nearby::plan(action, &rows) {
            self.nearby_media_op(op);
        }
    }

    /// The panel's actions since the last frame.
    pub(super) fn folderstorm_nearby_media_actions(&mut self) {
        for action in nearby::take_actions(&self.egui_ctx) {
            self.folderstorm_media_action(action);
        }
    }

    fn nearby_media_op(&mut self, op: Op) {
        match op {
            Op::PlayParcel => {
                self.media.play_parcel(&self.world, &self.settings.media);
                if let Some(m) = self.media.find_mut(&crate::media::MediaKey::Parcel) {
                    m.failed = None;
                }
            }
            Op::StopParcel => self.media.stop_parcel(),
            Op::Play(key) => {
                if let Some(m) = self.media.find_mut(&key) {
                    m.failed = None;
                    m.play();
                }
            }
            Op::Stop(key) => {
                if let Some(m) = self.media.find_mut(&key) {
                    m.wanted = false;
                }
                if self.media.focus == Some(key) {
                    self.media.focus = None;
                }
            }
            Op::SetMuted(key, muted) => {
                if let Some(m) = self.media.find_mut(&key) {
                    m.muted = muted;
                }
            }
            Op::TentativeAutoplay(on) => self.media.tentative_autoplay = on,
            Op::ToggleMusic => {
                self.parcel_music.toggle();
                self.remember_music_choice();
            }
            Op::OpenPreferences => {
                self.panels.settings = true;
                self.options_ui.open(ui::options::Page::Media, 0);
            }
        }
    }
}
