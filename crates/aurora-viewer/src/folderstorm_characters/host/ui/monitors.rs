//! The window's side of checkpoint 5, monitors (session-host plan,
//! section 12): in a character's card, the "Monitor" choice and, while it
//! is open, the latest picture of what that character's own viewer shows,
//! with its size and rate, labelled with the character, the picture's age
//! and how its viewer does.
//!
//! - **No flicker.** A picture is an egui texture, replaced whole when a
//!   new one comes (`TextureHandle::set`), and the window is painted off
//!   the screen and shown in one piece (`ui.rs`, the processor's painter).
//! - **Nothing reaches the viewer.** The picture senses neither clicks nor
//!   keys. The one way to act on that character is its card's "Switch to".
//! - **Nothing is kept.** The texture is freed when the host drops the
//!   picture (`monitors.rs`): no file, no copy, no earlier picture.
//! - **Seen or not.** Each drawing notes which monitors are on screen; a
//!   minimised or covered window sees none ([`seen_now`]). The desk asks
//!   pictures only of the viewers whose monitor is seen.

use super::Window_;
use crate::theme::Palette;
use crate::ui::widgets;
use ::folderstorm_session::host::{Health, Slot, Watch};
use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::monitor::{MAX_MONITORS, Rate, Size, Spec, State};
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::{Duration, Instant};

/// What the window keeps for the monitors.
#[derive(Default)]
pub(super) struct Panel {
    /// The pictures' textures.
    textures: [Option<egui::TextureHandle>; MAX_SLOTS],
    /// The monitors on screen in the last drawing.
    pub seen: [bool; MAX_SLOTS],
    /// The window is covered whole by others, as the system says.
    pub occluded: bool,
}

/// The monitors on screen now: those of the last drawing, none while the
/// window is minimised or covered.
pub(super) fn seen_now(seen: [bool; MAX_SLOTS], occluded: bool, minimised: bool) -> [bool; MAX_SLOTS] {
    if occluded || minimised { [false; MAX_SLOTS] } else { seen }
}

/// A rate as its choice shows it.
fn rate_text(rate: Rate) -> &'static str {
    match rate {
        Rate::Half => "0.5",
        Rate::One => "1",
        Rate::Two => "2",
        Rate::Five => "5",
    }
}

/// Pictures a second for a time between two pictures, as a text ("0.4").
fn per_second(interval_ms: u32) -> String {
    let rate = 1000.0 / f64::from(interval_ms.max(1));
    if rate >= 1.0 { format!("{rate:.1}") } else { format!("{rate:.2}") }
}

/// How old a picture is, in words.
fn age_text(age: Duration) -> String {
    match age.as_secs() {
        0..2 => tr!("folderstorm-monitor-age-now").to_owned(),
        seconds @ 2..120 => tr!("folderstorm-monitor-age", seconds = seconds),
        seconds => tr!("folderstorm-monitor-age-minutes", minutes = seconds / 60),
    }
}

/// How a monitor does, in words, and whether it is something to notice.
/// `pictured`: a picture is shown.
fn state_text(watch: Watch, viewer: Option<State>, health: Health, pictured: bool) -> (String, bool) {
    let (order, lowered) = match watch {
        Watch::Closed => return (String::new(), false),
        Watch::NoViewer => return (tr!("folderstorm-monitor-no-viewer").to_owned(), false),
        Watch::InFront => return (tr!("folderstorm-monitor-in-front").to_owned(), false),
        Watch::Hidden => return (tr!("folderstorm-monitor-hidden").to_owned(), false),
        Watch::TooMany => return (tr!("folderstorm-monitor-too-many", max = MAX_MONITORS), true),
        Watch::Asked { order, lowered } => (order, lowered),
    };
    if health == Health::NotAnswering {
        return (tr!("folderstorm-monitor-not-answering").to_owned(), true);
    }
    match viewer.unwrap_or_default() {
        State::Slowed => (tr!("folderstorm-monitor-slowed").to_owned(), true),
        State::CardBusy => (tr!("folderstorm-monitor-card-busy").to_owned(), true),
        State::Minimised => (tr!("folderstorm-monitor-minimised").to_owned(), true),
        State::NotInWorld => (tr!("folderstorm-monitor-not-in-world").to_owned(), false),
        State::Occupied => (tr!("folderstorm-monitor-occupied").to_owned(), false),
        State::Off | State::Live if !pictured => (tr!("folderstorm-monitor-waiting").to_owned(), false),
        State::Off | State::Live if lowered => (
            tr!("folderstorm-monitor-budget", rate = per_second(order.interval_ms)),
            false,
        ),
        State::Off | State::Live => (tr!("folderstorm-monitor-live").to_owned(), false),
    }
}

/// The largest rectangle of the picture's proportions inside `room`,
/// centred: the picture is never stretched.
fn fitted(room: egui::Rect, picture: [usize; 2]) -> egui::Rect {
    let (w, h) = (picture[0].max(1) as f32, picture[1].max(1) as f32);
    let scale = (room.width() / w).min(room.height() / h);
    egui::Rect::from_center_size(room.center(), Vec2::new(w * scale, h * scale))
}

impl Window_ {
    /// Before the characters are drawn: no monitor is seen yet, and the
    /// textures of pictures the host dropped are freed.
    pub(super) fn monitors_begin(&mut self) {
        self.monitors.seen = [false; MAX_SLOTS];
        for slot in 0..MAX_SLOTS {
            if self.host.screens[slot].shown.is_none() {
                self.monitors.textures[slot] = None;
            }
        }
    }

    /// Now and then: the desk learns which monitors are on screen.
    pub(super) fn monitors_watch(&mut self, now: Instant) {
        let minimised = self.window.as_ref().and_then(|w| w.is_minimized()).unwrap_or(false);
        let seen = seen_now(self.monitors.seen, self.monitors.occluded, minimised);
        self.host.desk.monitors_seen(seen, now);
    }

    /// In a character's row: its monitor open or not.
    pub(super) fn monitor_choice(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, acts: &mut Vec<super::Act>) {
        let mut open = state.monitor.is_some();
        if widgets::checkbox(ui, p, &mut open, tr!("folderstorm-monitor"))
            .on_hover_text(tr!("folderstorm-monitor-help"))
            .changed()
        {
            acts.push(super::Act::Monitor(slot, open.then(Spec::default)));
        }
    }

    /// Under a character's row, while its monitor is open: the size and
    /// rate, the picture, and its label.
    pub(super) fn monitor_panel(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        slot: usize,
        state: &Slot,
        now: Instant,
        acts: &mut Vec<super::Act>,
    ) {
        let Some(spec) = state.monitor else {
            return;
        };
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let sizes: Vec<String> = Size::ALL
                .iter()
                .map(|size| format!("{} × {}", size.dims().0, size.dims().1))
                .collect();
            let options: Vec<(&str, bool)> = sizes.iter().map(|text| (text.as_str(), true)).collect();
            let mut chosen = Size::ALL.iter().position(|size| *size == spec.size).unwrap_or(0);
            if widgets::segmented(ui, p, &mut chosen, &options) {
                let size = Size::ALL[chosen.min(Size::ALL.len() - 1)];
                acts.push(super::Act::Monitor(slot, Some(Spec { size, ..spec })));
            }
            let rates: Vec<String> = Rate::ALL
                .iter()
                .map(|rate| tr!("folderstorm-monitor-rate", rate = rate_text(*rate)))
                .collect();
            let options: Vec<(&str, bool)> = rates.iter().map(|text| (text.as_str(), true)).collect();
            let mut chosen = Rate::ALL.iter().position(|rate| *rate == spec.rate).unwrap_or(0);
            if widgets::segmented(ui, p, &mut chosen, &options) {
                let rate = Rate::ALL[chosen.min(Rate::ALL.len() - 1)];
                acts.push(super::Act::Monitor(slot, Some(Spec { rate, ..spec })));
            }
        });
        ui.add_space(6.0);
        // the box: the monitor's size in the screen's own pixels when the
        // card is wide enough, smaller otherwise, always 16:9
        let (box_width, box_height) = spec.size.dims();
        let width = (box_width as f32 / ui.ctx().pixels_per_point()).min(ui.available_width());
        let size = Vec2::new(width, (width * box_height as f32 / box_width as f32).round());
        // (hover only: no click and no key goes anywhere from here)
        let (room, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        self.monitors.seen[slot] = ui.is_rect_visible(room);
        let radius = ui.visuals().widgets.inactive.corner_radius;
        ui.painter().rect_filled(room, radius, p.navy);

        let screen = &mut self.host.screens[slot];
        let texture = &mut self.monitors.textures[slot];
        if let Some(image) = screen.fresh.take() {
            // the same texture, replaced whole: never a half-drawn picture
            match texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    let name = format!("folderstorm-monitor-{slot}");
                    *texture = Some(ui.ctx().load_texture(name, image, egui::TextureOptions::LINEAR));
                }
            }
        }
        let shown = screen.shown;
        let watch = self.host.desk.watch(slot);
        let viewer = state.status.as_ref().map(|status| status.monitor);
        let health = self.host.desk.health(slot, now);
        let (text, notice) = state_text(watch, viewer, health, shown.is_some());
        match (texture.as_ref(), shown) {
            (Some(texture), Some(shown)) => {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(texture.id(), fitted(room, shown.size), uv, egui::Color32::WHITE);
            }
            _ => {
                // no picture: why, in its place
                let galley = ui
                    .painter()
                    .layout(text.clone(), egui::FontId::proportional(13.0), p.muted, room.width() - 24.0);
                let at = room.center() - galley.size() / 2.0;
                ui.painter().galley(at, galley, p.muted);
            }
        }
        ui.add_space(4.0);
        let mut line = super::shown_name(state);
        if let Some(shown) = shown {
            line.push_str(" · ");
            line.push_str(&age_text(now.saturating_duration_since(shown.at)));
            if !text.is_empty() {
                line.push_str(" · ");
                line.push_str(&text);
            }
        }
        ui.label(RichText::new(line).color(if notice { p.amber } else { p.muted }));
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::tests::{draw, window};
    use super::*;
    use ::folderstorm_session::ipc::{Screen, Status};
    use ::folderstorm_session::monitor::Order;

    /// The two demo characters running, the first one active, both
    /// holding what the desk last asked of them.
    fn running(app: &mut Window_, now: Instant) {
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
    }

    fn report(app: &mut Window_, slot: usize, monitor: State, now: Instant) {
        let status = Status {
            screen: Screen::World,
            name: format!("Character {}", slot + 1),
            monitor,
            ..Default::default()
        };
        app.host.desk.status(slot, 1, status, now);
    }

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    fn order() -> Order {
        Order {
            size: Size::Medium,
            interval_ms: 1000,
        }
    }

    #[test]
    fn a_hidden_window_sees_no_monitor() {
        let seen = [true, false, true, true, false];
        assert_eq!(seen_now(seen, false, false), seen);
        assert_eq!(seen_now(seen, true, false), [false; MAX_SLOTS]);
        assert_eq!(seen_now(seen, false, true), [false; MAX_SLOTS]);
    }

    #[test]
    fn a_picture_keeps_its_proportions_in_the_box() {
        let room = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(480.0, 270.0));
        assert_eq!(fitted(room, [480, 270]), room);
        assert_eq!(fitted(room, [640, 360]), room, "scaled down whole");
        // a 4:3 window: as tall as the box, centred, bars left and right
        let narrow = fitted(room, [360, 270]);
        assert_eq!(narrow.size(), Vec2::new(360.0, 270.0));
        assert_eq!(narrow.center(), room.center());
        // wider than 16:9: bars above and below
        let wide = fitted(room, [640, 266]);
        assert!((wide.width() - 480.0).abs() < 0.01 && wide.height() < 270.0);
        assert_eq!(wide.center(), room.center());
    }

    #[test]
    fn the_label_says_the_age_and_how_the_viewer_does() {
        assert_eq!(age_text(Duration::from_millis(900)), tr!("folderstorm-monitor-age-now"));
        assert_eq!(age_text(Duration::from_secs(7)), tr!("folderstorm-monitor-age", seconds = 7));
        assert_eq!(age_text(Duration::from_secs(190)), tr!("folderstorm-monitor-age-minutes", minutes = 3));
        assert_eq!(per_second(1000), "1.0");
        assert_eq!(per_second(768), "1.3");
        assert_eq!(per_second(2500), "0.40");
        let asked = Watch::Asked {
            order: order(),
            lowered: false,
        };
        let text = |watch, viewer, health, pictured| state_text(watch, Some(viewer), health, pictured);
        assert_eq!(text(asked, State::Live, Health::Healthy, true), (tr!("folderstorm-monitor-live").to_owned(), false));
        assert_eq!(
            text(asked, State::Live, Health::Healthy, false).0,
            tr!("folderstorm-monitor-waiting"),
            "no picture yet"
        );
        // a busy card is said, and noticed
        assert_eq!(text(asked, State::Slowed, Health::Healthy, true), (tr!("folderstorm-monitor-slowed").to_owned(), true));
        assert_eq!(text(asked, State::CardBusy, Health::Healthy, true).0, tr!("folderstorm-monitor-card-busy"));
        assert_eq!(text(asked, State::Minimised, Health::Healthy, false).0, tr!("folderstorm-monitor-minimised"));
        assert_eq!(text(asked, State::NotInWorld, Health::Healthy, false).0, tr!("folderstorm-monitor-not-in-world"));
        // a viewer that does not answer comes before what it last said
        assert_eq!(
            text(asked, State::Live, Health::NotAnswering, true),
            (tr!("folderstorm-monitor-not-answering").to_owned(), true)
        );
        // the budget lowered the rate
        let lowered = Watch::Asked {
            order: Order {
                interval_ms: 768,
                ..order()
            },
            lowered: true,
        };
        assert_eq!(
            text(lowered, State::Live, Health::Healthy, true).0,
            tr!("folderstorm-monitor-budget", rate = "1.3")
        );
        assert_eq!(text(Watch::InFront, State::Off, Health::Healthy, false).0, tr!("folderstorm-monitor-in-front"));
        assert_eq!(text(Watch::Hidden, State::Off, Health::Healthy, true).0, tr!("folderstorm-monitor-hidden"));
    }

    #[test]
    fn a_monitor_is_asked_only_once_it_is_drawn_and_shows_the_latest_picture() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        report(&mut app, 0, State::Off, now);
        report(&mut app, 1, State::Off, now);
        // closed: no picture box, nothing asked
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, tr!("folderstorm-monitor")));
        assert_eq!(app.monitors.seen, [false; MAX_SLOTS]);
        app.monitors_watch(now);
        assert_eq!(app.host.desk.watch(1), Watch::Closed);
        // opened: drawn, then seen, then asked of its viewer
        app.host.desk.set_monitor(1, Some(Spec::default()));
        assert_eq!(app.host.desk.watch(1), Watch::Hidden);
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(app.monitors.seen[1]);
        assert!(shows(&drawn, "480 × 270"));
        app.monitors_watch(now);
        assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, tr!("folderstorm-monitor-waiting")));
        assert!(app.monitors.textures[1].is_none());
        // a picture comes: a texture, the character's name and its age
        let red = egui::Color32::from_rgb(200, 30, 30);
        app.host.test_picture(1, [480, 270], red, now);
        report(&mut app, 1, State::Live, now);
        let (drawn, pixels) = draw(&mut app, 1120, 1100);
        assert!(app.monitors.textures[1].is_some());
        assert!(app.host.screens[1].fresh.is_none(), "taken for the texture");
        assert!(shows(&drawn, "Character 2"));
        assert!(shows(&drawn, tr!("folderstorm-monitor-age-now")), "{drawn:?}");
        assert!(shows(&drawn, tr!("folderstorm-monitor-live")));
        let reds = |pixels: &[u32]| pixels.iter().filter(|p| **p == 0x00C8_1E1E).count();
        assert!(reds(&pixels) > 480 * 270 / 2, "the picture is painted: {} red pixels", reds(&pixels));
        // the next one replaces it in the same texture: no second texture,
        // nothing of the first one left on screen
        let id = app.monitors.textures[1].as_ref().map(egui::TextureHandle::id);
        let green = egui::Color32::from_rgb(20, 180, 60);
        app.host.test_picture(1, [480, 270], green, now);
        let (_, pixels) = draw(&mut app, 1120, 1100);
        assert_eq!(app.monitors.textures[1].as_ref().map(egui::TextureHandle::id), id);
        assert_eq!(reds(&pixels), 0);
        assert!(pixels.iter().filter(|p| **p == 0x0014_B43C).count() > 480 * 270 / 2);
        // a busy card is said under the picture
        report(&mut app, 1, State::Slowed, now);
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, tr!("folderstorm-monitor-slowed")));
    }

    #[test]
    fn a_hidden_host_stops_asking_and_a_closed_monitor_keeps_nothing() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.desk.set_monitor(1, Some(Spec::default()));
        draw(&mut app, 1120, 1100);
        app.monitors_watch(now);
        assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
        app.host.test_picture(1, [480, 270], egui::Color32::WHITE, now);
        draw(&mut app, 1120, 1100);
        assert!(app.monitors.textures[1].is_some());
        // the window is covered, or minimised: its viewer is told to stop
        app.host.desk.take_effects();
        app.monitors.occluded = true;
        app.monitors_watch(now);
        assert_eq!(app.host.desk.watch(1), Watch::Hidden);
        let stopped = app.host.desk.take_effects().iter().any(|effect| {
            matches!(
                effect,
                ::folderstorm_session::host::Effect::Send {
                    slot: 1,
                    message: ::folderstorm_session::ipc::ToViewer::Assign(a),
                    ..
                } if a.role == ::folderstorm_session::ipc::Role::Background
            )
        });
        assert!(stopped, "the viewer is a plain background character again");
        // back: asked again, the last picture still there with its age
        app.monitors.occluded = false;
        app.monitors_watch(now);
        assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
        app.host.tick_monitors(now);
        assert!(app.host.screens[1].shown.is_some());
        // the chat's page of a narrow window shows no monitor
        app.chat_page = true;
        draw(&mut app, 600, 1100);
        assert_eq!(app.monitors.seen, [false; MAX_SLOTS]);
        app.chat_page = false;
        draw(&mut app, 600, 1100);
        assert!(app.monitors.seen[1]);
        // closed: the picture and its texture are gone at once
        app.host.desk.set_monitor(1, None);
        app.host.tick_monitors(now);
        assert!(app.host.screens[1].shown.is_none() && app.host.screens[1].fresh.is_none());
        draw(&mut app, 1120, 1100);
        assert!(app.monitors.textures[1].is_none());
        assert_eq!(app.host.desk.watch(1), Watch::Closed);
    }

    #[test]
    fn the_character_in_front_and_one_that_left_the_world_keep_no_picture() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        for slot in 0..2 {
            app.host.desk.set_monitor(slot, Some(Spec::default()));
        }
        draw(&mut app, 1120, 1100);
        app.monitors_watch(now);
        app.host.test_picture(1, [480, 270], egui::Color32::WHITE, now);
        app.host.tick_monitors(now);
        assert!(app.host.screens[1].shown.is_some());
        // the active character's monitor says it is in front
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, tr!("folderstorm-monitor-in-front")));
        // it leaves the world: the picture of where it was is dropped
        report(&mut app, 1, State::NotInWorld, now);
        app.host.tick_monitors(now);
        assert!(app.host.screens[1].shown.is_none());
        // it comes to the front: nothing is kept either
        report(&mut app, 1, State::Live, now);
        app.host.test_picture(1, [480, 270], egui::Color32::WHITE, now);
        app.host.desk.switch(1, now);
        app.host.tick_monitors(now);
        assert!(app.host.screens[1].shown.is_none());
        assert_eq!(app.host.desk.watch(1), Watch::InFront);
    }
}
