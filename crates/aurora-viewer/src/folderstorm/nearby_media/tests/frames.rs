//! The top bar's speaker, radio and media buttons drawn frame by frame (Aurora's
//! `ui::audio::top_controls`, with the Folderstorm hook): the media button's
//! face, its click, and the Nearby media panel its hover opens.
//! `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps a picture
//! (`nearby-media.png`).

use super::super::panel::{self, CLOSE_DELAY};
use super::super::{Action, Filter, Place, Row, Snapshot, Source, State};
use super::prim;
use crate::settings::AudioSettings;
use crate::ui::audio::{AudioAction, AudioUi, MediaState, MusicStatus};
use aurora_i18n::tr;
use egui::{Pos2, Rect, pos2, vec2};

const WIDTH: u32 = 520;
const HEIGHT: u32 = 380;
/// Where the bar's three buttons are drawn, right to left.
const BAR: Rect = Rect::from_min_max(pos2(380.0, 6.0), pos2(500.0, 24.0));
/// The speaker, the radio and the media button (20 px slots, 2 px apart).
const SPEAKER: Pos2 = pos2(490.0, 15.0);
const MEDIA: Pos2 = pos2(446.0, 15.0);

struct Bench {
    ctx: egui::Context,
    p: crate::theme::Palette,
    icons: crate::ui::icons::Icons,
    audio: AudioSettings,
    audio_ui: AudioUi,
    media: MediaState,
    snapshot: Snapshot,
    time: f64,
    texts: Vec<(String, Rect)>,
    /// Textures drawn over the media button this frame.
    button_textures: Vec<egui::TextureId>,
    actions: Vec<AudioAction>,
    pixels: Vec<u32>,
    painter: crate::crash::software::Painter,
    pointer: Pos2,
}

fn media_row(source: Source, name: &str, on: bool, distance: f32) -> Row {
    Row {
        source,
        name: name.into(),
        url: format!("https://{}.example/", name.to_lowercase().replace(' ', "-")),
        state: if on { State::Playing } else { State::Stopped },
        on,
        muted: false,
        distance,
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

fn seeded() -> Snapshot {
    Snapshot {
        rows: vec![
            media_row(Source::ParcelMedia, tr!("fs-nearby-media-parcel-media"), true, 0.0),
            media_row(Source::ParcelMusic, tr!("fs-nearby-media-parcel-music"), false, 0.0),
            media_row(prim(1), "Club screen", true, 6.0),
            media_row(prim(2), "Gallery slideshow", false, 18.0),
        ],
        media_enabled: true,
        plugins_missing: false,
        show_on_others: false,
    }
}

/// [`seeded`] with a billboard outside the parcel and a badge worn by
/// another avatar standing on it.
fn with_neighbours() -> Snapshot {
    let mut s = seeded();
    let mut billboard = media_row(prim(3), "Billboard next door", true, 66.0);
    billboard.place.in_parcel = false;
    let mut badge = media_row(prim(4), "Wolf badge", true, 5.0);
    badge.place.on_other_avatar = true;
    s.rows.extend([billboard, badge]);
    s
}

impl Bench {
    fn new() -> Self {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let mut bench = Bench {
            ctx,
            p,
            icons,
            audio: AudioSettings::default(),
            audio_ui: AudioUi::default(),
            media: MediaState {
                music_available: true,
                music_playing: false,
                music_status: MusicStatus::Stopped,
                media_available: true,
                media_playing: true,
            },
            snapshot: seeded(),
            time: 0.0,
            texts: Vec::new(),
            button_textures: Vec::new(),
            actions: Vec::new(),
            pixels: vec![0; (WIDTH * HEIGHT) as usize],
            painter: Default::default(),
            pointer: pos2(5.0, 300.0),
        };
        bench.frames(2);
        bench
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(WIDTH as f32, HEIGHT as f32));
        self.time += 1.0 / 60.0;
        let (p, media) = (self.p, self.media);
        let (icons, audio, audio_ui) = (&self.icons, &mut self.audio, &mut self.audio_ui);
        let snapshot = self.snapshot.clone();
        let mut action = AudioAction::None;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                // what the app does before it draws the bar
                if super::super::wants_snapshot(ui.ctx()) {
                    super::super::publish(ui.ctx(), snapshot.clone());
                }
                let mut bar = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(BAR)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                action = crate::ui::audio::top_controls(&mut bar, &p, icons, audio, audio_ui, media, crate::ui::audio::BAR_SLOT);
            },
        );
        self.actions.push(action);
        self.texts.clear();
        self.button_textures.clear();
        let button = Rect::from_center_size(MEDIA, vec2(20.0, 18.0));
        fn collect(shape: &egui::Shape, button: Rect, texts: &mut Vec<(String, Rect)>, textures: &mut Vec<egui::TextureId>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, button, texts, textures)),
                egui::Shape::Text(t) => texts.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Mesh(m) if m.vertices.iter().all(|v| button.contains(v.pos)) => textures.push(m.texture_id),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            collect(&clipped.shape, button, &mut self.texts, &mut self.button_textures);
        }
        let shapes = std::mem::take(&mut output.shapes);
        let primitives = self.ctx.tessellate(shapes, output.pixels_per_point);
        self.pixels.fill(0);
        self.painter.paint(
            &mut self.pixels,
            [WIDTH as usize, HEIGHT as usize],
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            self.p.navy,
        );
        output.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![egui::Event::PointerMoved(self.pointer)]);
        }
    }

    fn move_to(&mut self, at: Pos2) {
        self.pointer = at;
        self.frames(3);
    }

    fn click(&mut self, at: Pos2) {
        self.pointer = at;
        self.frame(vec![egui::Event::PointerMoved(at)]);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
        self.frames(1);
    }

    fn shown(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _)| t == text)
    }

    fn starts(&self, prefix: &str) -> Option<&str> {
        self.texts.iter().map(|(t, _)| t.as_str()).find(|t| t.starts_with(prefix))
    }

    /// The centre of a drawn text (the footer's buttons carry their label).
    fn at(&self, text: &str) -> Pos2 {
        self.texts
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| r.center())
            .unwrap_or_else(|| panic!("“{text}” drawn: {:?}", self.texts))
    }

    /// The centre of a drawn text starting with `prefix`.
    fn at_start(&self, prefix: &str) -> Pos2 {
        self.texts
            .iter()
            .find(|(t, _)| t.starts_with(prefix))
            .map(|(_, r)| r.center())
            .unwrap_or_else(|| panic!("“{prefix}…” drawn: {:?}", self.texts))
    }

    fn double_click(&mut self, at: Pos2) {
        // well after any earlier click, or egui counts a triple click
        self.time += 1.0;
        self.pointer = at;
        self.frame(vec![egui::Event::PointerMoved(at)]);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
        self.frames(1);
    }

    fn button(&self, source: Source, kind: &'static str) -> Pos2 {
        panel::button_rect(&self.ctx, source, kind)
            .unwrap_or_else(|| panic!("{kind} button of {source:?} drawn"))
            .center()
    }

    /// Opens the Show list and picks `filter` in it.
    fn choose(&mut self, filter: Filter) {
        let (combo, _) = panel::filter_rects(&self.ctx, filter);
        self.click(combo.expect("the Show list drawn").center());
        let (_, item) = panel::filter_rects(&self.ctx, filter);
        self.click(item.expect("the Show list's entries drawn").center());
    }

    /// The rows' names drawn.
    fn listed(&self) -> Vec<&str> {
        ["Club screen", "Gallery slideshow", "Billboard next door", "Wolf badge"]
            .into_iter()
            .chain([tr!("fs-nearby-media-parcel-media"), tr!("fs-nearby-media-parcel-music")])
            .filter(|name| self.starts(name).is_some())
            .collect()
    }

    fn face(&self, icon: &str) -> bool {
        let id = self.icons.get(icon).expect("icon").id();
        self.button_textures.contains(&id)
    }

    fn capture(&self, name: &str) {
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = self
                .pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::save_buffer(path, &rgba, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("capture written");
        }
    }
}

#[test]
fn the_buttons_face_shows_whether_media_plays() {
    let mut bench = Bench::new();
    assert!(bench.face("player-pause-filled"));
    assert!(!bench.face("brand-youtube-filled"));
    bench.media.media_playing = false;
    bench.frames(1);
    assert!(bench.face("brand-youtube-filled"));
    assert!(!bench.face("player-pause-filled"));
}

#[test]
fn a_click_on_the_button_toggles_all_media() {
    let mut bench = Bench::new();
    bench.click(MEDIA);
    assert_eq!(bench.actions.iter().filter(|a| **a == AudioAction::ToggleMedia).count(), 1);
    // nothing to play: the click does nothing
    bench.actions.clear();
    bench.media.media_available = false;
    bench.click(MEDIA);
    assert!(!bench.actions.contains(&AudioAction::ToggleMedia));
}

#[test]
fn hovering_the_button_lists_the_nearby_media() {
    let mut bench = Bench::new();
    assert!(!bench.shown(tr!("fs-nearby-media-title")));
    bench.move_to(MEDIA);
    bench.capture("nearby-media");
    for text in [
        tr!("fs-nearby-media-title"),
        tr!("fs-nearby-media-parcel-media"),
        tr!("fs-nearby-media-parcel-music"),
        tr!("fs-nearby-media-state-playing"),
        tr!("fs-nearby-media-state-stopped"),
        tr!("fs-nearby-media-stop-all"),
        tr!("fs-nearby-media-start-all"),
    ] {
        assert!(bench.shown(text), "“{text}” shown: {:?}", bench.texts);
    }
    // the objects' media with their distance
    let club = bench.starts("Club screen").expect("the club's screen listed");
    assert!(club.contains(&tr!("fs-nearby-media-distance", metres = "6")), "{club}");
    assert!(bench.starts("Gallery slideshow").is_some());
    // the volume panel stays closed
    assert!(!bench.audio_ui.panel_open);
}

#[test]
fn the_rows_buttons_ask_for_their_own_source() {
    let mut bench = Bench::new();
    bench.move_to(MEDIA);
    let stop = bench.button(prim(1), "toggle");
    bench.click(stop);
    let start = bench.button(prim(2), "toggle");
    bench.click(start);
    let music = bench.button(Source::ParcelMusic, "toggle");
    bench.click(music);
    let mute = bench.button(Source::ParcelMedia, "mute");
    bench.click(mute);
    assert_eq!(
        super::super::take_actions(&bench.ctx),
        [
            Action::Toggle(prim(1)),
            Action::Toggle(prim(2)),
            Action::Toggle(Source::ParcelMusic),
            Action::Mute(Source::ParcelMedia),
        ]
    );
    // the music has no mute of its own here
    assert!(panel::button_rect(&bench.ctx, Source::ParcelMusic, "mute").is_none());
    // the panel stayed open through the clicks
    assert!(bench.shown(tr!("fs-nearby-media-title")));
}

#[test]
fn stop_all_and_start_all_are_queued() {
    let mut bench = Bench::new();
    bench.move_to(MEDIA);
    // the footer's buttons: under the list, Stop all first
    let stop_all = bench.at(tr!("fs-nearby-media-stop-all"));
    bench.click(stop_all);
    let start_all = bench.at(tr!("fs-nearby-media-start-all"));
    bench.click(start_all);
    assert_eq!(super::super::take_actions(&bench.ctx), [Action::StopAll, Action::StartAll]);
}

#[test]
fn the_speaker_takes_over_and_leaving_closes_the_panel() {
    let mut bench = Bench::new();
    bench.move_to(MEDIA);
    assert!(bench.shown(tr!("fs-nearby-media-title")));
    bench.move_to(SPEAKER);
    assert!(!bench.shown(tr!("fs-nearby-media-title")));
    assert!(bench.audio_ui.panel_open);
    // back on the media button: the volume panel gives way
    bench.move_to(MEDIA);
    assert!(bench.shown(tr!("fs-nearby-media-title")));
    assert!(!bench.audio_ui.panel_open);
    // away from both: closed once the delay is over
    bench.pointer = pos2(5.0, 360.0);
    bench.frames(1);
    assert!(bench.shown(tr!("fs-nearby-media-title")));
    bench.time += CLOSE_DELAY + 0.1;
    bench.frames(2);
    assert!(!bench.shown(tr!("fs-nearby-media-title")));
}

#[test]
fn media_turned_off_says_so_and_keeps_the_rows_still() {
    let mut bench = Bench::new();
    bench.snapshot.media_enabled = false;
    bench.move_to(MEDIA);
    assert!(bench.shown(tr!("fs-nearby-media-disabled")));
    let stop = bench.button(prim(1), "toggle");
    bench.click(stop);
    assert!(super::super::take_actions(&bench.ctx).is_empty());
    // the music is not a media: its row still works
    let music = bench.button(Source::ParcelMusic, "toggle");
    bench.click(music);
    assert_eq!(super::super::take_actions(&bench.ctx), [Action::Toggle(Source::ParcelMusic)]);
}

#[test]
fn every_glyph_of_the_button_and_the_rows_exists() {
    let bench = Bench::new();
    for icon in [
        panel::button_icon(true),
        panel::button_icon(false),
        panel::source_icon(Source::ParcelMedia),
        panel::source_icon(Source::ParcelMusic),
        panel::source_icon(prim(1)),
        "player-play-filled",
        "player-stop-filled",
        "volume",
        "volume-off",
        "settings-filled",
        "zoom-in-filled",
        "zoom-out-filled",
    ] {
        assert!(bench.icons.get(icon).is_some(), "{icon}");
    }
}

#[test]
fn the_show_list_keeps_the_rows_of_one_place() {
    let mut bench = Bench::new();
    bench.snapshot = with_neighbours();
    bench.move_to(MEDIA);
    assert_eq!(panel::filter(&bench.ctx), Filter::All);
    assert!(bench.shown(Filter::All.label()));
    assert_eq!(bench.listed().len(), 6, "{:?}", bench.texts);
    bench.capture("nearby-media-all");
    bench.choose(Filter::OutsideParcel);
    bench.capture("nearby-media-outside");
    assert_eq!(bench.listed(), ["Billboard next door"]);
    // the panel stayed open through the list's clicks
    assert!(bench.shown(tr!("fs-nearby-media-title")));
    assert!(bench.shown(Filter::OutsideParcel.label()));
    bench.choose(Filter::WithinParcel);
    let mut within = bench.listed();
    within.sort_unstable();
    let mut expected = vec![
        "Club screen",
        "Gallery slideshow",
        "Wolf badge",
        tr!("fs-nearby-media-parcel-media"),
        tr!("fs-nearby-media-parcel-music"),
    ];
    expected.sort_unstable();
    assert_eq!(within, expected, "{:?}", bench.texts);
    // on other avatars: the badge, and why the list may stay empty
    bench.choose(Filter::OnOthers);
    assert_eq!(bench.listed(), ["Wolf badge"]);
    assert!(bench.shown(tr!("fs-nearby-media-others-off")));
    bench.snapshot.show_on_others = true;
    bench.frames(1);
    assert!(!bench.shown(tr!("fs-nearby-media-others-off")));
    // kept once the panel closed and opened again
    bench.pointer = pos2(5.0, 360.0);
    bench.frames(1);
    bench.time += CLOSE_DELAY + 0.1;
    bench.frames(2);
    assert!(!bench.shown(tr!("fs-nearby-media-title")));
    bench.move_to(MEDIA);
    assert_eq!(bench.listed(), ["Wolf badge"]);
    // nothing of that place: said so, not "no media nearby"
    bench.snapshot = seeded();
    bench.frames(1);
    assert!(bench.shown(tr!("fs-nearby-media-empty-filter")));
    assert!(!bench.shown(tr!("fs-nearby-media-empty")));
}

#[test]
fn zoom_is_offered_for_media_on_objects_only() {
    let mut bench = Bench::new();
    bench.snapshot = with_neighbours();
    // the badge's avatar is out of view: greyed
    bench.snapshot.rows[5].place.zoomable = false;
    bench.move_to(MEDIA);
    // the parcel's media and music have none
    assert!(panel::button_rect(&bench.ctx, Source::ParcelMedia, "zoom").is_none());
    assert!(panel::button_rect(&bench.ctx, Source::ParcelMusic, "zoom").is_none());
    let zoom = bench.button(prim(3), "zoom");
    bench.click(zoom);
    let greyed = bench.button(prim(4), "zoom");
    bench.click(greyed);
    assert_eq!(super::super::take_actions(&bench.ctx), [Action::Zoom(prim(3))]);
    // the camera on it: the same button zooms back out
    bench.snapshot.rows[4].place.zoomed = true;
    bench.frames(1);
    let unzoom = bench.button(prim(3), "zoom");
    bench.click(unzoom);
    assert_eq!(super::super::take_actions(&bench.ctx), [Action::Unzoom]);
    // a double click on a name zooms on it too
    let name = bench.at_start("Club screen");
    bench.double_click(name);
    assert_eq!(super::super::take_actions(&bench.ctx), [Action::Zoom(prim(1))]);
}

#[test]
fn a_rows_slider_sets_the_volume_of_its_own_source() {
    let mut bench = Bench::new();
    bench.move_to(MEDIA);
    let slider = panel::button_rect(&bench.ctx, prim(2), "volume").expect("the gallery's slider");
    bench.click(pos2(slider.left() + slider.width() * 0.3, slider.center().y));
    let actions = super::super::take_actions(&bench.ctx);
    assert!(!actions.is_empty());
    for action in &actions {
        match action {
            Action::SetVolume(source, volume) => {
                assert_eq!(*source, prim(2));
                assert!((0.0..0.9).contains(volume), "{volume}");
            }
            other => panic!("only the gallery's volume: {other:?}"),
        }
    }
    // the music's volume is the volume panel's
    assert!(panel::button_rect(&bench.ctx, Source::ParcelMusic, "volume").is_none());
    assert!(panel::button_rect(&bench.ctx, Source::ParcelMedia, "volume").is_some());
    // media turned off: the slider stays still
    bench.snapshot.media_enabled = false;
    bench.frames(1);
    bench.click(pos2(slider.left() + slider.width() * 0.6, slider.center().y));
    assert!(super::super::take_actions(&bench.ctx).is_empty());
}
