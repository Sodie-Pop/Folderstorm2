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
        volume: 1.0,
        place: match source {
            Source::Prim { .. } => Place {
                in_parcel: true,
                zoomable: true,
                ..Default::default()
            },
            _ => Place::PARCEL,
        },
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
fn the_bar_button_stops_everything_while_anything_plays_and_starts_it_all_otherwise() {
    let rows = rows();
    assert_eq!(plan(Action::ToggleAll, &rows), plan(Action::StopAll, &rows));
    // only the music on: the media are off, the button starts them
    let quiet: Vec<_> = rows
        .iter()
        .cloned()
        .map(|mut r| {
            r.on = r.source == Source::ParcelMusic;
            r
        })
        .collect();
    assert_eq!(
        plan(Action::ToggleAll, &quiet),
        [Op::TentativeAutoplay(true), Op::PlayParcel, Op::Play(key(1)), Op::Play(key(2))]
    );
}

#[test]
fn stop_all_and_start_all_are_offered_when_they_would_change_something() {
    let mut s = Snapshot {
        rows: rows(),
        media_enabled: true,
        plugins_missing: false,
        show_on_others: false,
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
fn the_button_shows_whether_media_plays_and_acts_on_all_media() {
    use std::sync::Arc as StdArc;
    let mut world = crate::world::World::new(StdArc::new(crate::scene::avatar::AvatarLibrary::load()));
    let settings = MediaSettings::default();
    let mut media = MediaManager::default();
    assert!(!available(&media, None, &settings));
    assert!(!playing(&media, &settings));
    let parcel = aurora_net::ParcelInfo {
        media_url: "https://parcel.example/".into(),
        media: aurora_net::ParcelMedia {
            mime: "text/html".into(),
            media_id: id(77),
            ..Default::default()
        },
        ..Default::default()
    };
    let url = parcel.media_url.clone();
    world.parcel = Some(StdArc::new(parcel));
    assert!(available(&media, Some(&url), &settings));
    // what the bar's Start all does to the parcel media
    media.play_parcel(&world, &settings);
    assert!(playing(&media, &settings));
    assert_eq!(panel::button_icon(true), "player-pause-filled");
    media.stop_parcel();
    assert!(!playing(&media, &settings));
    assert_eq!(panel::button_icon(false), "brand-youtube-filled");
    // media turned off in the preferences: nothing to toggle
    let off = MediaSettings {
        enabled: false,
        ..Default::default()
    };
    assert!(!available(&media, Some(&url), &off));
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

fn placed(source: Source, in_parcel: bool, on_other_avatar: bool) -> Row {
    let mut r = row(source, true);
    r.place.in_parcel = in_parcel;
    r.place.on_other_avatar = on_other_avatar;
    r
}

#[test]
fn the_show_list_keeps_the_rows_of_their_place_and_the_parcels_own_with_all_and_within() {
    let rows = [
        row(Source::ParcelMedia, true),
        row(Source::ParcelMusic, true),
        placed(prim(1), true, false),
        placed(prim(2), false, false),
        // worn by another avatar standing on our parcel, then away from it
        placed(prim(3), true, true),
        placed(prim(4), false, true),
    ];
    let shown = |f: Filter| rows.iter().filter(|r| f.shows(r)).map(|r| r.source).collect::<Vec<_>>();
    assert_eq!(shown(Filter::All).len(), 6);
    assert_eq!(
        shown(Filter::WithinParcel),
        [Source::ParcelMedia, Source::ParcelMusic, prim(1), prim(3)]
    );
    assert_eq!(shown(Filter::OutsideParcel), [prim(2), prim(4)]);
    assert_eq!(shown(Filter::OnOthers), [prim(3), prim(4)]);
    // the list's order and labels, All first
    assert_eq!(Filter::default(), Filter::All);
    assert_eq!(Filter::ALL[0], Filter::All);
    assert_eq!(Filter::OnOthers.label(), aurora_i18n::tr!("fs-nearby-media-filter-others"));
}

#[test]
fn a_media_is_in_the_agents_parcel_when_its_object_stands_on_its_cells() {
    // the demo parcel: x and y from 64 to 192 m (cells 16..48)
    let mut cells = vec![0u8; 512];
    for y in 16..48 {
        for x in 16..48 {
            let i = y * 64 + x;
            cells[i / 8] |= 1 << (i % 8);
        }
    }
    let at = |x: f32, y: f32| Some(glam::Vec3::new(x, y, 25.0));
    assert!(place::in_agent_parcel(false, at(135.5, 129.5), Some(&cells)));
    assert!(place::in_agent_parcel(false, at(64.0, 191.9), Some(&cells)));
    assert!(!place::in_agent_parcel(false, at(198.0, 131.0), Some(&cells)));
    assert!(!place::in_agent_parcel(false, at(-5.0, 100.0), Some(&cells)));
    // another region (outside 0..256): never the agent's parcel
    assert!(!place::in_agent_parcel(false, at(300.0, 100.0), Some(&cells)));
    // a HUD rides with us
    assert!(place::in_agent_parcel(true, None, None));
    // nothing known: not in it, as the Legacy Viewer without its parcel
    assert!(!place::in_agent_parcel(false, at(135.5, 129.5), None));
    assert!(!place::in_agent_parcel(false, None, Some(&cells)));
}

/// The demo world with the Nearby media demo's billboard and badge.
fn demo_world() -> crate::world::World {
    let mut world = crate::world::World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
    for event in crate::demo::events() {
        world.apply(event);
    }
    world.agent_id = crate::demo::DEMO_AGENT;
    for (event, ..) in demo::sources() {
        world.apply(event);
    }
    world
}

#[test]
fn the_demos_billboard_is_outside_the_parcel_and_the_badge_on_another_avatar() {
    let world = demo_world();
    assert!(world.parcel.is_some(), "the demo parcel");
    let now = std::time::Instant::now();
    let billboard = place::place_of(&world, None, demo::OUTSIDE, now);
    assert_eq!(
        billboard,
        Place {
            in_parcel: false,
            on_other_avatar: false,
            zoomable: true,
            zoomed: false
        }
    );
    // Violet Wolf stands on the parcel: both classes, as there
    let badge = place::place_of(&world, None, demo::WORN, now);
    assert!(badge.on_other_avatar && badge.in_parcel, "{badge:?}");
    // the demo's own screen, on the plaza
    let (event, screen, _) = crate::demo::media_screen();
    let mut world = world;
    world.apply(event);
    let plaza = place::place_of(&world, None, screen, now);
    assert!(plaza.in_parcel && !plaza.on_other_avatar && plaza.zoomable, "{plaza:?}");
    // an object not (or no longer) in the world
    assert_eq!(place::place_of(&world, None, id(404), now), Place::default());
}

#[test]
fn a_zoom_frames_the_media_object_and_the_camera_keeps_it_until_unzoomed() {
    let mut world = demo_world();
    let mut camera = crate::camera::Camera::default();
    let settings = crate::camera::CameraSettings::default();
    let now = std::time::Instant::now();
    let idx = world.objects.index_of_uuid(&demo::OUTSIDE).expect("billboard");
    let (centre, ..) = crate::scene::Scene::object_transform(&world, idx, now, 0).expect("placed");
    assert!(place::zoom_on(&mut camera, &mut world, demo::OUTSIDE, None, 1.6, &settings, now));
    assert_eq!(camera.focus_object_id(), Some(demo::OUTSIDE));
    let focus = camera.focus_point().expect("a focus point");
    assert!(focus.distance(centre) < 0.5, "{focus} {centre}");
    // the row now offers to zoom back out
    assert!(place::place_of(&world, camera.focus_object_id(), demo::OUTSIDE, now).zoomed);
    assert!(!place::place_of(&world, camera.focus_object_id(), demo::WORN, now).zoomed);
    place::unzoom(&mut camera, &mut world, &settings);
    assert_eq!(camera.focus_object_id(), None);
    // nothing to frame
    assert!(!place::zoom_on(&mut camera, &mut world, id(404), None, 1.6, &settings, now));
}

#[test]
fn a_face_box_turns_with_its_object() {
    let model = glam::Mat4::from_rotation_translation(
        glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        glam::Vec3::new(10.0, 20.0, 30.0),
    );
    let (centre, size) = place::world_box(model, glam::Vec3::new(1.0, 0.0, 0.0), glam::Vec3::new(0.5, 2.0, 1.0));
    assert!(centre.distance(glam::Vec3::new(10.0, 21.0, 30.0)) < 1e-4, "{centre}");
    assert!(size.distance(glam::Vec3::new(4.0, 1.0, 2.0)) < 1e-4, "{size}");
}

#[test]
fn the_volume_slider_and_the_zoom_ask_for_their_own_source() {
    let mut rows = rows();
    assert_eq!(plan(Action::SetVolume(prim(2), 0.25), &rows), [Op::SetVolume(key(2), 0.25)]);
    assert_eq!(
        plan(Action::SetVolume(Source::ParcelMedia, 1.7), &rows),
        [Op::SetVolume(MediaKey::Parcel, 1.0)]
    );
    // the music's volume is the volume panel's; a source gone: nothing
    assert!(plan(Action::SetVolume(Source::ParcelMusic, 0.5), &rows).is_empty());
    assert!(plan(Action::SetVolume(prim(9), 0.5), &rows).is_empty());
    assert_eq!(plan(Action::Zoom(prim(1)), &rows), [Op::Zoom { object: id(1), face: 0 }]);
    // the parcel's media and music have no zoom, nor an object out of view
    assert!(plan(Action::Zoom(Source::ParcelMedia), &rows).is_empty());
    assert!(plan(Action::Zoom(Source::ParcelMusic), &rows).is_empty());
    rows[3].place.zoomable = false;
    assert!(plan(Action::Zoom(prim(2)), &rows).is_empty());
    assert_eq!(plan(Action::Unzoom, &rows), [Op::Unzoom]);
}

#[test]
fn a_row_carries_its_sources_volume_and_place() {
    let settings = MediaSettings::default();
    let near = MediaFacts {
        volume: 0.4,
        place: Place {
            in_parcel: false,
            zoomable: true,
            ..Default::default()
        },
        ..facts(key(1), "", "https://a.example/", 3.0)
    };
    let s = snapshot(
        [near],
        Some("https://parcel.example/"),
        music("http://radio.example/", false, MusicStatus::Stopped),
        &settings,
        false,
    );
    // the parcel media not started yet: full volume, in the parcel, no zoom
    assert_eq!((s.rows[0].volume, s.rows[0].place), (1.0, Place::PARCEL));
    assert_eq!(s.rows[1].place, Place::PARCEL);
    assert_eq!(s.rows[2].volume, 0.4);
    assert!(!s.rows[2].place.in_parcel && s.rows[2].place.zoomable);
    assert!(!s.show_on_others);
}
