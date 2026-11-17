//! Folderstorm: the top bar's media button as the Legacy Viewer's
//! `media_toggle_btn` (LLStatusBar): its face says whether any media
//! plays, and hovering it opens "Nearby media" (LLPanelNearByMedia,
//! panel_nearby_media.xml): each source with its own start / stop and mute,
//! Stop all, Start all and a way to the Media preferences. Its click, all
//! media off or back on, is Aurora's own (`App::toggle_all_media`), as is
//! the nearby media panel of `ui/nearby_media.rs`, which this one replaces
//! on hover (the hook in `ui/audio.rs`).
//!
//! As in the Legacy Viewer, which separates the parcel stream from the media
//! (FS:Zi "Media/Stream separation"), the parcel music keeps its own radio
//! button and is left alone by Stop all and Start all; it
//! still has its row in the panel, to start or stop it from there too.
//!
//! The app hands a [`Snapshot`] to the panel through egui's memory
//! ([`publish`]) and runs the [`Action`]s the panel queues ([`take_actions`])
//! as [`Op`]s ([`plan`]) on Aurora's own media manager and radio
//! (app/folderstorm_nearby_media.rs). Aurora's top bar reaches it through
//! one hook in `ui/audio.rs`.

pub mod panel;
#[cfg(test)]
mod tests;

use crate::media::{MediaImpl, MediaKey, MediaSettings};
use crate::ui::audio::MusicStatus;
use aurora_media::MediaStatus;
use std::sync::Arc;
use uuid::Uuid;

/// One row of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The parcel's media (PARCEL_MEDIA_LIST_ITEM_UUID).
    ParcelMedia,
    /// The parcel's music stream (PARCEL_AUDIO_LIST_ITEM_UUID).
    ParcelMusic,
    /// Media on a face of an object (shared media), HUDs included.
    Prim { object: Uuid, face: u8 },
}

impl Source {
    /// The media manager's key of a media source; `None` for the music.
    pub fn key(self) -> Option<MediaKey> {
        match self {
            Source::ParcelMedia => Some(MediaKey::Parcel),
            Source::ParcelMusic => None,
            Source::Prim { object, face } => Some(MediaKey::Prim { object, face }),
        }
    }

    fn is_media(self) -> bool {
        self != Source::ParcelMusic
    }
}

/// What a row says about its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    /// The media filter waits for the resident's answer about the site.
    Waiting,
    /// Wanted, not playing yet (plugin starting, page loading, no free
    /// plugin slot).
    Starting,
    Playing,
    Paused,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub source: Source,
    /// The page's title, else its address, else a placeholder.
    pub name: String,
    /// The address, for the row's tooltip.
    pub url: String,
    pub state: State,
    /// Started (and not paused): the row's button stops it.
    pub on: bool,
    pub muted: bool,
    /// Metres from the camera (0 for the parcel and HUDs).
    pub distance: f32,
    /// A longer word on the state (the music's reason), for the tooltip.
    pub note: String,
}

/// What the panel shows, built by the app while the panel is open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub rows: Vec<Row>,
    /// Preferences › Media › Enable media (AudioStreamingMedia).
    pub media_enabled: bool,
    /// No media plugins found yet: web pages and videos cannot load.
    pub plugins_missing: bool,
}

impl Snapshot {
    fn media_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.source.is_media())
    }

    /// Stop all: some media is on (mDisableAllCtrl).
    pub fn can_stop_all(&self) -> bool {
        self.media_enabled && self.media_rows().any(|r| r.on)
    }

    /// Start all: some media is off (mEnableAllCtrl).
    pub fn can_start_all(&self) -> bool {
        self.media_enabled && self.media_rows().any(|r| !r.on)
    }
}

/// What the panel asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The row's start / stop button (onClickSelectedMediaPlay / Stop).
    Toggle(Source),
    /// The row's mute button (onClickSelectedMediaMute).
    Mute(Source),
    StopAll,
    StartAll,
    OpenPreferences,
}

/// One change for the app to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// LLViewerParcelMedia::play (through the media filter).
    PlayParcel,
    /// LLViewerParcelMedia::stop: unloads it.
    StopParcel,
    /// setDisabled(false): wanted again, a failure forgotten.
    Play(MediaKey),
    /// setDisabled(true): unloaded, and not started again by itself.
    Stop(MediaKey),
    SetMuted(MediaKey, bool),
    /// MediaTentativeAutoPlay: new media on objects may start by itself.
    TentativeAutoplay(bool),
    /// The radio button's toggle.
    ToggleMusic,
    OpenPreferences,
}

/// The ops of an action, from the rows as they are now.
pub fn plan(action: Action, rows: &[Row]) -> Vec<Op> {
    let row = |source: Source| rows.iter().find(|r| r.source == source);
    let start = |r: &Row| match r.source {
        Source::ParcelMedia => Some(Op::PlayParcel),
        Source::ParcelMusic => None,
        Source::Prim { object, face } => Some(Op::Play(MediaKey::Prim { object, face })),
    };
    let stop = |r: &Row| match r.source {
        Source::ParcelMedia => Some(Op::StopParcel),
        Source::ParcelMusic => None,
        Source::Prim { object, face } => Some(Op::Stop(MediaKey::Prim { object, face })),
    };
    let set_all = |on: bool| {
        let mut ops = vec![Op::TentativeAutoplay(on)];
        for r in rows.iter().filter(|r| r.source.is_media() && r.on != on) {
            ops.extend(if on { start(r) } else { stop(r) });
        }
        ops
    };
    match action {
        Action::Toggle(Source::ParcelMusic) => row(Source::ParcelMusic).map(|_| Op::ToggleMusic).into_iter().collect(),
        Action::Toggle(source) => row(source)
            .and_then(|r| if r.on { stop(r) } else { start(r) })
            .into_iter()
            .collect(),
        Action::Mute(source) => row(source)
            .and_then(|r| source.key().map(|key| Op::SetMuted(key, !r.muted)))
            .into_iter()
            .collect(),
        Action::StopAll => set_all(false),
        Action::StartAll => set_all(true),
        Action::OpenPreferences => vec![Op::OpenPreferences],
    }
}

/// What the panel reads of one media source (an Aurora `MediaImpl`).
#[derive(Debug, Clone, Default)]
pub struct MediaFacts<'a> {
    pub key: Option<MediaKey>,
    pub title: &'a str,
    /// The page shown, the entry's current page, its home page
    /// (getNameAndUrlHelper's order).
    pub urls: [&'a str; 3],
    pub wanted: bool,
    pub paused: bool,
    pub failed: bool,
    pub filter_hold: bool,
    pub loaded: bool,
    pub status: MediaStatus,
    pub muted: bool,
    pub distance: f32,
}

impl<'a> MediaFacts<'a> {
    pub fn of(m: &'a MediaImpl) -> Self {
        let entry = m.entry.as_ref();
        MediaFacts {
            key: Some(m.key),
            title: &m.title,
            urls: [
                if m.location.is_empty() { &m.url } else { &m.location },
                entry.map_or("", |e| e.current_url.as_str()),
                entry.map_or("", |e| e.home_url.as_str()),
            ],
            wanted: m.wanted,
            paused: m.paused,
            failed: m.failed.is_some(),
            filter_hold: m.filter_hold,
            loaded: m.is_loaded(),
            status: m.status,
            muted: m.muted,
            distance: m.distance,
        }
    }

    fn state(&self) -> State {
        if !self.wanted {
            State::Stopped
        } else if self.failed {
            State::Failed
        } else if self.filter_hold {
            State::Waiting
        } else if self.paused {
            State::Paused
        } else if self.loaded && matches!(self.status, MediaStatus::Playing | MediaStatus::Loaded | MediaStatus::Done) {
            State::Playing
        } else {
            State::Starting
        }
    }
}

/// The parcel's music, as the radio button knows it.
#[derive(Debug, Clone, Copy)]
pub struct MusicFacts<'a> {
    pub station: &'a str,
    pub requested: bool,
    pub status: MusicStatus,
}

/// The name and the address of a source (getNameAndUrlHelper): its title,
/// else the first address known; the address without its query for the
/// name, whole for the tooltip.
pub fn name_and_url(title: &str, urls: [&str; 3]) -> (String, String) {
    let url = urls.iter().map(|u| u.trim()).find(|u| !u.is_empty()).unwrap_or_default();
    let title = title.trim();
    let name = if !title.is_empty() {
        title.to_owned()
    } else if !url.is_empty() {
        short_url(url)
    } else {
        aurora_i18n::tr!("fs-nearby-media-unnamed").to_owned()
    };
    (name, url.to_owned())
}

/// An address as a name: no scheme, no query, no fragment.
fn short_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    rest.trim_end_matches('/').to_owned()
}

fn music_state(music: &MusicFacts) -> State {
    if !music.requested {
        return State::Stopped;
    }
    match music.status {
        MusicStatus::Playing | MusicStatus::Muted => State::Playing,
        MusicStatus::Waiting => State::Waiting,
        MusicStatus::Connecting => State::Starting,
        MusicStatus::Error(_) | MusicStatus::Blocked | MusicStatus::NoDevice | MusicStatus::Disabled => State::Failed,
        MusicStatus::Stopped | MusicStatus::Unavailable => State::Stopped,
    }
}

/// The rows (refreshParcelItems, then refreshList): the parcel's media
/// when it has some, its music when it has a station, then the media on
/// objects, nearest first.
pub fn snapshot<'a>(
    media: impl IntoIterator<Item = MediaFacts<'a>>,
    parcel_url: Option<&str>,
    music: MusicFacts,
    settings: &MediaSettings,
    plugins_missing: bool,
) -> Snapshot {
    let mut parcel = None;
    let mut prims = Vec::new();
    for facts in media {
        match facts.key {
            Some(MediaKey::Parcel) => parcel = Some(facts),
            Some(MediaKey::Prim { object, face }) => {
                let (name, url) = name_and_url(facts.title, facts.urls);
                prims.push(Row {
                    source: Source::Prim { object, face },
                    name,
                    url,
                    state: facts.state(),
                    on: facts.wanted && !facts.paused,
                    muted: facts.muted,
                    distance: facts.distance,
                    note: String::new(),
                });
            }
            None => {}
        }
    }
    prims.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let mut rows = Vec::with_capacity(prims.len() + 2);
    if let Some(url) = parcel_url.map(str::trim).filter(|u| !u.is_empty()) {
        let facts = parcel.unwrap_or_default();
        rows.push(Row {
            source: Source::ParcelMedia,
            name: aurora_i18n::tr!("fs-nearby-media-parcel-media").to_owned(),
            url: url.to_owned(),
            state: facts.state(),
            on: facts.wanted && !facts.paused,
            muted: facts.muted,
            distance: 0.0,
            note: String::new(),
        });
    }
    if !music.station.trim().is_empty() || music.requested {
        rows.push(Row {
            source: Source::ParcelMusic,
            name: aurora_i18n::tr!("fs-nearby-media-parcel-music").to_owned(),
            url: music.station.trim().to_owned(),
            state: music_state(&music),
            on: music.requested,
            muted: false,
            distance: 0.0,
            note: music.status.text(),
        });
    }
    rows.extend(prims);
    Snapshot {
        rows,
        media_enabled: settings.enabled,
        plugins_missing,
    }
}

// ---------------------------------------------------------- egui's memory

/// The snapshot of this frame, with the pass it was made for.
#[derive(Clone)]
struct Published(Arc<Snapshot>, u64);

fn snapshot_id() -> egui::Id {
    egui::Id::unique("fs_nearby_media_snapshot")
}

fn actions_id() -> egui::Id {
    egui::Id::unique("fs_nearby_media_actions")
}

/// The panel is open (or about to be): the app should publish a snapshot.
pub fn wants_snapshot(ctx: &egui::Context) -> bool {
    panel::is_open(ctx)
}

/// The app's snapshot for the panel, before the top bar is drawn.
pub fn publish(ctx: &egui::Context, snapshot: Snapshot) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(snapshot_id(), Published(Arc::new(snapshot), pass)));
}

/// The snapshot published for this pass, if any.
fn current(ctx: &egui::Context) -> Option<Arc<Snapshot>> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Published>(snapshot_id()))
        .filter(|p| p.1 == pass)
        .map(|p| p.0)
}

fn push_action(ctx: &egui::Context, action: Action) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Action>>(actions_id()).push(action));
}

/// What the panel asked for since the last call.
pub fn take_actions(ctx: &egui::Context) -> Vec<Action> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Action>>(actions_id())).unwrap_or_default()
}
