//! Folderstorm: the top bar's media button and its Nearby media panel in
//! the app (the rules are in `crate::folderstorm::nearby_media`). The panel's
//! actions run on Aurora's own media manager, radio and camera, as the
//! Legacy Viewer's LLPanelNearByMedia does on LLViewerMedia and
//! LLViewerMediaFocus: a media source is started through the media filter as
//! the parcel media button and a click on a face do, the music as the radio
//! button does, and a zoom frames the face as a click on a "zoom" object.

use super::*;
use crate::folderstorm::nearby_media::{self as nearby, Action, MediaFacts, MusicFacts, Op, Snapshot, place};

impl App {
    /// The rows of the Nearby media panel as they are now.
    fn nearby_media_snapshot(&self) -> Snapshot {
        let now = Instant::now();
        let focus = self.camera.focus_object_id();
        nearby::snapshot(
            self.media.impls.iter().map(|m| {
                let facts = MediaFacts::of(m);
                match m.key {
                    crate::media::MediaKey::Prim { object, .. } => MediaFacts {
                        place: place::place_of(&self.world, focus, object, now),
                        ..facts
                    },
                    crate::media::MediaKey::Parcel => facts,
                }
            }),
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
        if self.demo && nearby::demo::active() {
            self.nearby_media_demo();
        }
        for action in nearby::take_actions(&self.egui_ctx) {
            self.folderstorm_media_action(action);
        }
    }

    /// `AURORA_DEMO_NEARBY_MEDIA`: the demo's extra sources, once their
    /// objects are missing (at the first frame, and after the demo world
    /// was rebuilt).
    fn nearby_media_demo(&mut self) {
        if self.world.objects.index_of_uuid(&nearby::demo::OUTSIDE).is_some() {
            return;
        }
        self.settings.media.show_on_others = true;
        for (ev, object, face, page) in nearby::demo::sources() {
            if let Some(e) = self.world.apply(ev) {
                self.on_app_event(e);
            }
            self.media.demo_face(object, face, page, false);
        }
    }

    /// The box of a media face in the world (centre, size), from the
    /// geometry drawn for its object.
    fn nearby_media_face_box(&self, idx: usize, face: u8, now: Instant) -> Option<(Vec3, Vec3)> {
        let (model, geom) = self.scene.drawn_geometry(&self.world, idx, now)?;
        let bounds = geom.pick_faces.get(face as usize)?.as_ref()?.bounds;
        Some(place::world_box(model, bounds.center, bounds.half))
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
            Op::SetVolume(key, volume) => {
                if let Some(m) = self.media.find_mut(&key) {
                    m.volume = volume;
                }
            }
            Op::Zoom { object, face } => {
                let now = Instant::now();
                let face_box = self.world.objects.index_of_uuid(&object).and_then(|idx| {
                    self.nearby_media_face_box(idx, face, now)
                        .or_else(|| self.scene.click_bounds(&self.world, idx))
                });
                let aspect = self.view_aspect();
                place::zoom_on(
                    &mut self.camera,
                    &mut self.world,
                    object,
                    face_box,
                    aspect,
                    &self.settings.camera,
                    now,
                );
            }
            Op::Unzoom => place::unzoom(&mut self.camera, &mut self.world, &self.settings.camera),
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
