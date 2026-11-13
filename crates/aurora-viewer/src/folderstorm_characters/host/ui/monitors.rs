//! The window's side of checkpoint 5, monitors (session-host plan,
//! section 12): the latest picture of what a background character's own
//! viewer shows, with its size and rate, the picture's age and how its
//! viewer does.
//!
//! - **No flicker.** A picture is an egui texture, replaced whole when a
//!   new one comes (`TextureHandle::set`), and the window is painted off
//!   the screen and shown in one piece (`ui.rs`, the processor's painter).
//! - **Nothing reaches the viewer.** The picture senses neither clicks nor
//!   keys. The one way to act on that character is "Switch".
//! - **Nothing is kept.** The texture is freed when the host drops the
//!   picture (`monitors.rs`): no file, no copy, no earlier picture.
//! - **Seen or not.** Each drawing notes which monitors are on screen; a
//!   minimised or covered window sees none (`panels::seen`). The desk
//!   asks pictures only of the viewers whose monitor is seen.
//! - **Each monitor has a window of its own** (checkpoint 9,
//!   `monitor_windows.rs`), which draws the picture; a character's card
//!   keeps the "Monitor" box, which opens and closes that window, and the
//!   size and rate. (Checkpoint 6's one window for all monitors, and the
//!   pictures in the cards, are gone.)

use super::Window_;
use super::monitor_windows::Seat;
use crate::theme::Palette;
use crate::ui::widgets;
use ::folderstorm_session::host::{Health, Slot, Watch};
use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::monitor::{MAX_MONITORS, Rate, Size, Spec, State};
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::{Duration, Instant};

/// What one window keeps for the monitors.
#[derive(Default)]
pub(super) struct Panel {
    /// The pictures' textures in this window, each with the count of the
    /// picture it holds (`monitors::Screen::serial`).
    textures: [Option<(u64, egui::TextureHandle)>; MAX_SLOTS],
    /// The monitors on screen in the window's last drawing.
    pub seen: [bool; MAX_SLOTS],
}

impl Panel {
    /// The texture of a character's picture in this window.
    pub fn texture(&self, slot: usize) -> Option<&egui::TextureHandle> {
        self.textures.get(slot)?.as_ref().map(|(_, texture)| texture)
    }

    /// The host's latest picture as this window's texture: the same
    /// texture, replaced whole when the picture is a new one (never a
    /// half-drawn picture), made when this window had none.
    pub(super) fn show(&mut self, ctx: &egui::Context, slot: usize, serial: u64, picture: Option<std::sync::Arc<egui::ColorImage>>) {
        let Some(picture) = picture else {
            return;
        };
        match &mut self.textures[slot] {
            Some((held, _)) if *held == serial => {}
            Some((held, texture)) => {
                texture.set(picture, egui::TextureOptions::LINEAR);
                *held = serial;
            }
            none => {
                let name = format!("folderstorm-monitor-{slot}");
                *none = Some((serial, ctx.load_texture(name, picture, egui::TextureOptions::LINEAR)));
            }
        }
    }
}

/// A rate as its choice shows it.
pub(super) fn rate_text(rate: Rate) -> &'static str {
    match rate {
        Rate::Half => "0.5",
        Rate::One => "1",
        Rate::Two => "2",
        Rate::Five => "5",
    }
}

/// Pictures a second for a time between two pictures, as a text ("0.4").
pub(super) fn per_second(interval_ms: u32) -> String {
    let rate = 1000.0 / f64::from(interval_ms.max(1));
    if rate >= 1.0 { format!("{rate:.1}") } else { format!("{rate:.2}") }
}

/// How old a picture is, in words.
pub(super) fn age_text(age: Duration) -> String {
    match age.as_secs() {
        0..2 => tr!("folderstorm-monitor-age-now").to_owned(),
        seconds @ 2..120 => tr!("folderstorm-monitor-age", seconds = seconds),
        seconds => tr!("folderstorm-monitor-age-minutes", minutes = seconds / 60),
    }
}

/// How a monitor does, in words, and whether it is something to notice.
/// `pictured`: a picture is shown.
pub(super) fn state_text(watch: Watch, viewer: Option<State>, health: Health, pictured: bool) -> (String, bool) {
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
        State::Off | State::Live if lowered => (tr!("folderstorm-monitor-budget", rate = per_second(order.interval_ms)), false),
        State::Off | State::Live => (tr!("folderstorm-monitor-live").to_owned(), false),
    }
}

/// The largest rectangle of the picture's proportions inside `room`,
/// centred: the picture is never stretched.
pub(super) fn fitted(room: egui::Rect, picture: [usize; 2]) -> egui::Rect {
    let (w, h) = (picture[0].max(1) as f32, picture[1].max(1) as f32);
    let scale = (room.width() / w).min(room.height() / h);
    egui::Rect::from_center_size(room.center(), Vec2::new(w * scale, h * scale))
}

impl Window_ {
    /// Before a window is drawn: no monitor is seen in it yet, and it
    /// keeps no texture of a picture the host dropped or that it does
    /// not draw (a monitor's window draws its character's, the cycling
    /// monitor's the one whose turn it is).
    pub(super) fn monitors_begin(&mut self) {
        let drawn = match Seat::of(self.drawing) {
            Some(Seat::Slot(slot)) => Some(slot),
            Some(Seat::Cycling) => self.monitors.cycle.shown(),
            None => None,
        };
        let kept: [bool; MAX_SLOTS] = std::array::from_fn(|slot| drawn == Some(slot) && self.host.screens[slot].shown.is_some());
        let Some(view) = self.view_mut(self.drawing) else {
            return;
        };
        view.monitors.seen = [false; MAX_SLOTS];
        for (texture, kept) in view.monitors.textures.iter_mut().zip(kept) {
            if !kept {
                *texture = None;
            }
        }
    }

    /// Now and then: the cycling monitor's turn, then the desk learns
    /// which monitors are on screen, in any of their windows that is not
    /// minimised or covered, and whom the cycling monitor shows.
    pub(super) fn monitors_watch(&mut self, now: Instant) {
        // (a monitor that changed character since the last look has its
        // window first: what is seen is said for the right one)
        self.follow_moves();
        self.cycling_tick(now);
        let sights: Vec<_> = self
            .monitors
            .views()
            .filter(|(seat, _)| *seat != Seat::Cycling)
            .map(|(_, view)| view.sight())
            .collect();
        self.host.desk.monitors_seen(::folderstorm_session::panels::seen(&sights), now);
        self.host.desk.cycling_seen(self.cycling_sight(), now);
    }

    /// In a character's row: its monitor open or not (its window opens
    /// with the size and rate a new monitor has).
    pub(super) fn monitor_choice(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, acts: &mut Vec<super::Act>) {
        let mut open = state.monitor.is_some();
        if widgets::checkbox(ui, p, &mut open, tr!("folderstorm-monitor"))
            .on_hover_text(tr!("folderstorm-monitor-help"))
            .changed()
        {
            let defaults = self.host.desk.monitors().defaults;
            acts.push(super::Act::Monitor(slot, open.then_some(defaults)));
        }
    }

    /// Under a character's row, while its monitor is open: the size and
    /// rate, and where the picture is (its own window).
    pub(super) fn monitor_panel(&mut self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, acts: &mut Vec<super::Act>) {
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
        ui.label(RichText::new(tr!("folderstorm-monitor-in-window")).color(p.muted));
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::tests::{draw, window};
    use super::*;
    use ::folderstorm_session::monitor::Order;

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
        assert_eq!(
            age_text(Duration::from_secs(190)),
            tr!("folderstorm-monitor-age-minutes", minutes = 3)
        );
        assert_eq!(per_second(1000), "1.0");
        assert_eq!(per_second(768), "1.3");
        assert_eq!(per_second(2500), "0.40");
        let asked = Watch::Asked {
            order: order(),
            lowered: false,
        };
        let text = |watch, viewer, health, pictured| state_text(watch, Some(viewer), health, pictured);
        assert_eq!(
            text(asked, State::Live, Health::Healthy, true),
            (tr!("folderstorm-monitor-live").to_owned(), false)
        );
        assert_eq!(
            text(asked, State::Live, Health::Healthy, false).0,
            tr!("folderstorm-monitor-waiting"),
            "no picture yet"
        );
        // a busy card is said, and noticed
        assert_eq!(
            text(asked, State::Slowed, Health::Healthy, true),
            (tr!("folderstorm-monitor-slowed").to_owned(), true)
        );
        assert_eq!(
            text(asked, State::CardBusy, Health::Healthy, true).0,
            tr!("folderstorm-monitor-card-busy")
        );
        assert_eq!(
            text(asked, State::Minimised, Health::Healthy, false).0,
            tr!("folderstorm-monitor-minimised")
        );
        assert_eq!(
            text(asked, State::NotInWorld, Health::Healthy, false).0,
            tr!("folderstorm-monitor-not-in-world")
        );
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
        assert_eq!(
            text(Watch::InFront, State::Off, Health::Healthy, false).0,
            tr!("folderstorm-monitor-in-front")
        );
        assert_eq!(
            text(Watch::Hidden, State::Off, Health::Healthy, true).0,
            tr!("folderstorm-monitor-hidden")
        );
    }

    #[test]
    fn a_card_opens_its_character_s_monitor_and_says_where_the_picture_is() {
        let now = Instant::now();
        let mut app = window(host());
        // closed: the box, no size or rate
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, tr!("folderstorm-monitor")));
        assert!(!shows(&drawn, "480 × 270"));
        // the box opens it with the size and rate a new monitor has
        let large = Spec {
            size: Size::Large,
            rate: Rate::Two,
        };
        app.host.desk.edit_monitors(|monitors| monitors.defaults = large);
        let defaults = app.host.desk.monitors().defaults;
        app.apply(vec![super::super::Act::Monitor(1, Some(defaults))], now);
        assert_eq!(app.host.desk.slot(1).and_then(|s| s.monitor), Some(large));
        // open: its size and rate, and no picture in the host's window
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(
            shows(&drawn, "640 × 360") && shows(&drawn, tr!("folderstorm-monitor-in-window")),
            "{drawn:?}"
        );
        assert_eq!(app.main.monitors.seen, [false; MAX_SLOTS]);
        app.host.test_picture(1, [640, 360], egui::Color32::from_rgb(200, 30, 30), now);
        let (_, pixels) = draw(&mut app, 1120, 1100);
        assert!(app.main.monitors.texture(1).is_none());
        assert_eq!(pixels.iter().filter(|p| **p == 0x00C8_1E1E).count(), 0);
    }
}
