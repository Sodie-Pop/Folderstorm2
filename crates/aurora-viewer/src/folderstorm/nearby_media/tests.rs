//! The media button's rules: the rows of Nearby media, what each action
//! changes, and the panel's hover.

mod frames;

use super::panel::{CLOSE_DELAY, Hover};
use super::*;
use crate::media::MediaKey;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn prim(n: u128) -> Source {
    Source::Prim { object: id(n), face: 0 }
}

fn key(n: u128) -> MediaKey {
    MediaKey::Prim { object: id(n), face: 0 }
}

fn row(source: Source, on: bool) -> Row {
    Row {
        source,
        name: format!("{source:?}"),
        url: String::new(),
        state: if on { State::Playing } else { State::Stopped },
        on,
        muted: false,
        distance: 0.0,
        note: String::new(),
    }
}

/// Parcel media on, music on, one prim on, one off.
fn rows() -> Vec<Row> {
    vec![
        row(Source::ParcelMedia, true),
        row(Source::ParcelMusic, true),
        row(prim(1), true),
        row(prim(2), false),
    ]
}

#[test]
fn a_row_button_starts_or_stops_its_own_source() {
    let rows = rows();
    assert_eq!(plan(Action::Toggle(prim(1)), &rows), [Op::Stop(key(1))]);
    assert_eq!(plan(Action::Toggle(prim(2)), &rows), [Op::Play(key(2))]);
    assert_eq!(plan(Action::Toggle(Source::ParcelMedia), &rows), [Op::StopParcel]);
    let mut off = rows.clone();
    off[0].on = false;
    assert_eq!(plan(Action::Toggle(Source::ParcelMedia), &off), [Op::PlayParcel]);
    // the music goes through the radio button's own toggle
    assert_eq!(plan(Action::Toggle(Source::ParcelMusic), &rows), [Op::ToggleMusic]);
    // a source gone since the panel was drawn: nothing
    assert!(plan(Action::Toggle(prim(9)), &rows).is_empty());
    assert!(plan(Action::Toggle(Source::ParcelMusic), &rows[2..]).is_empty());
}

#[test]
fn the_mute_button_flips_the_sources_own_mute() {
    let mut rows = rows();
    assert_eq!(plan(Action::Mute(prim(2)), &rows), [Op::SetMuted(key(2), true)]);
    rows[0].muted = true;
    assert_eq!(
        plan(Action::Mute(Source::ParcelMedia), &rows),
        [Op::SetMuted(MediaKey::Parcel, false)]
    );
    // the music's volume is the volume panel's
    assert!(plan(Action::Mute(Source::ParcelMusic), &rows).is_empty());
}

#[test]
fn stop_all_and_start_all_change_the_media_and_leave_the_music() {
    let rows = rows();
    assert_eq!(
        plan(Action::StopAll, &rows),
        [Op::TentativeAutoplay(false), Op::StopParcel, Op::Stop(key(1))]
    );
    assert_eq!(plan(Action::StartAll, &rows), [Op::TentativeAutoplay(true), Op::Play(key(2))]);
    let mut off = rows.clone();
    off[0].on = false;
    assert_eq!(
        plan(Action::StartAll, &off),
        [Op::TentativeAutoplay(true), Op::PlayParcel, Op::Play(key(2))]
    );
    assert_eq!(plan(Action::OpenPreferences, &rows), [Op::OpenPreferences]);
}

#[test]
fn stop_all_and_start_all_are_offered_when_they_would_change_something() {
    let mut s = Snapshot {
        rows: rows(),
        media_enabled: true,
        plugins_missing: false,
    };
    assert!(s.can_stop_all() && s.can_start_all());
    s.rows[3].on = true;
    assert!(s.can_stop_all() && !s.can_start_all());
    for r in &mut s.rows {
        r.on = r.source == Source::ParcelMusic;
    }
    // the music alone does not count
    assert!(!s.can_stop_all() && s.can_start_all());
    s.media_enabled = false;
    assert!(!s.can_stop_all() && !s.can_start_all());
}

fn facts(key: MediaKey, title: &'static str, url: &'static str, distance: f32) -> MediaFacts<'static> {
    MediaFacts {
        key: Some(key),
        title,
        urls: [url, "", ""],
        wanted: true,
        loaded: true,
        status: MediaStatus::Playing,
        distance,
        ..Default::default()
    }
}

fn music(station: &'static str, requested: bool, status: MusicStatus) -> MusicFacts<'static> {
    MusicFacts {
        station,
        requested,
        status,
    }
}

#[test]
fn the_rows_are_the_parcel_media_the_music_then_the_objects_nearest_first() {
    let settings = MediaSettings::default();
    let media = [
        facts(key(1), "", "https://far.example/page?token=1", 30.0),
        facts(MediaKey::Parcel, "", "https://parcel.example/", 0.0),
        facts(key(2), "Near screen", "https://near.example/", 4.0),
    ];
    let s = snapshot(
        media,
        Some("https://parcel.example/"),
        music("http://radio.example/stream", false, MusicStatus::Stopped),
        &settings,
        false,
    );
    let sources: Vec<_> = s.rows.iter().map(|r| r.source).collect();
    assert_eq!(sources, [Source::ParcelMedia, Source::ParcelMusic, prim(2), prim(1)]);
    assert_eq!(s.rows[0].name, aurora_i18n::tr!("fs-nearby-media-parcel-media"));
    assert_eq!(s.rows[0].url, "https://parcel.example/");
    assert!(s.rows[0].on && s.rows[0].state == State::Playing);
    assert_eq!(s.rows[1].name, aurora_i18n::tr!("fs-nearby-media-parcel-music"));
    assert!(!s.rows[1].on && s.rows[1].state == State::Stopped);
    // a title first, else the address without scheme or query
    assert_eq!(s.rows[2].name, "Near screen");
    assert_eq!(s.rows[3].name, "far.example/page");
    assert_eq!(s.rows[3].url, "https://far.example/page?token=1");
    assert!(s.media_enabled && !s.plugins_missing);
}

#[test]
fn a_parcel_without_media_or_music_has_no_row_for_them() {
    let settings = MediaSettings::default();
    let s = snapshot([], Some("  "), music("", false, MusicStatus::Unavailable), &settings, true);
    assert!(s.rows.is_empty());
    assert!(s.plugins_missing);
    // parcel media not started yet: its row, stopped
    let s = snapshot(
        [],
        Some("https://parcel.example/"),
        music("", false, MusicStatus::Unavailable),
        &settings,
        false,
    );
    assert_eq!(s.rows.len(), 1);
    assert!(!s.rows[0].on && s.rows[0].state == State::Stopped);
}

#[test]
fn a_rows_state_follows_the_filter_the_plugin_and_the_pause() {
    let base = facts(key(1), "", "https://a.example/", 1.0);
    let state = |f: MediaFacts| {
        snapshot(
            [f],
            None,
            music("", false, MusicStatus::Unavailable),
            &MediaSettings::default(),
            false,
        )
        .rows[0]
            .state
    };
    assert_eq!(state(base.clone()), State::Playing);
    assert_eq!(
        state(MediaFacts {
            wanted: false,
            ..base.clone()
        }),
        State::Stopped
    );
    assert_eq!(
        state(MediaFacts {
            filter_hold: true,
            ..base.clone()
        }),
        State::Waiting
    );
    assert_eq!(
        state(MediaFacts {
            failed: true,
            ..base.clone()
        }),
        State::Failed
    );
    assert_eq!(
        state(MediaFacts {
            loaded: false,
            ..base.clone()
        }),
        State::Starting
    );
    let paused = snapshot(
        [MediaFacts {
            paused: true,
            ..base.clone()
        }],
        None,
        music("", false, MusicStatus::Unavailable),
        &MediaSettings::default(),
        false,
    );
    // a paused media is not on: its button plays it again
    assert_eq!(paused.rows[0].state, State::Paused);
    assert!(!paused.rows[0].on);
}

#[test]
fn the_music_row_follows_the_radio() {
    let settings = MediaSettings::default();
    let row = |requested, status| snapshot([], None, music("http://radio.example/", requested, status), &settings, false).rows[0].clone();
    assert_eq!(row(true, MusicStatus::Playing).state, State::Playing);
    assert_eq!(row(true, MusicStatus::Connecting).state, State::Starting);
    assert_eq!(row(true, MusicStatus::Waiting).state, State::Waiting);
    let failed = row(true, MusicStatus::Error(aurora_audio::StreamError::Connection));
    assert_eq!(failed.state, State::Failed);
    // the radio's reason, for the tooltip
    assert_eq!(failed.note, MusicStatus::Error(aurora_audio::StreamError::Connection).text());
    let stopped = row(false, MusicStatus::Stopped);
    assert!(!stopped.on && stopped.state == State::Stopped);
    assert_eq!(stopped.url, "http://radio.example/");
}

#[test]
fn a_name_is_the_title_then_the_first_address_then_a_placeholder() {
    assert_eq!(
        name_and_url("  Club  ", ["", "https://a.example/x", ""]),
        ("Club".into(), "https://a.example/x".into())
    );
    assert_eq!(
        name_and_url("", ["", "", "http://home.example/#top"]),
        ("home.example".into(), "http://home.example/#top".into())
    );
    assert_eq!(
        name_and_url("", ["", "", ""]),
        (aurora_i18n::tr!("fs-nearby-media-unnamed").to_owned(), String::new())
    );
}

#[test]
fn the_button_face_shows_whether_media_plays() {
    assert_eq!(panel::button_icon(true), "player-pause-filled");
    assert_eq!(panel::button_icon(false), "brand-youtube-filled");
}

#[test]
fn the_panel_opens_on_hover_and_closes_after_the_delay() {
    let mut h = Hover::default();
    assert!(!h.step(false, false, false, false, 0.0));
    assert!(!h.open);
    h.step(true, false, false, false, 1.0);
    assert!(h.open);
    // on the way down to the panel
    assert!(h.step(false, false, false, false, 1.1));
    assert!(h.step(false, false, false, false, 1.1 + CLOSE_DELAY - 0.01));
    assert!(h.open);
    h.step(false, true, false, false, 1.3);
    h.step(false, true, false, false, 9.0);
    assert!(h.open);
    assert!(h.step(false, false, false, false, 10.0));
    assert!(!h.step(false, false, false, false, 10.0 + CLOSE_DELAY + 0.01));
    assert!(!h.open);
    // a held button keeps it open but does not open it
    h.step(false, false, true, false, 11.0);
    assert!(!h.open);
}

#[test]
fn the_speaker_takes_over_at_once() {
    let mut h = Hover::default();
    h.step(true, false, false, false, 0.0);
    assert!(h.open);
    assert!(!h.step(false, false, false, true, 0.1));
    assert!(!h.open);
}
