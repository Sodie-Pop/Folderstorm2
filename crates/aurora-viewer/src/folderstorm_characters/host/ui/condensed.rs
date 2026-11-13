//! The window's side of checkpoint 7's condensed layout (session-host
//! plan, section 13): the characters one line each (colour, alias or
//! name, where its viewer is, what is wrong, voice, monitor, unread
//! lines, what waits for an answer, and its quick action) in place of
//! the cards. The choice is the desk's (`condensed`, in
//! `session-host.json`); the quick action is checkpoint 4's
//! (`Desk::quick`), the same one the cards' strip and the hotkeys press.
//!
//! It is the characters' panel drawn another way: it works wherever that
//! panel is, in the host's window or in its own, and beside or without
//! the other panels. What the cards alone hold (standby, sound, monitor,
//! alias and colour, close, end, remove, add, the hotkeys' and the new
//! profile's pages) is one click away: "Cards". The host's questions
//! (closing it while characters run, "Quit all" under way) are asked
//! here as there.
//!
//! The monitors' pictures are in the cards: in this layout a monitor
//! that is not in the monitors' own window is not drawn, so it asks its
//! viewer for nothing (3D Off) until the cards or that window show it.

use super::super::faces;
use super::comfort::quick_face;
use super::{Act, Window_, comfort, status_now, warnings};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::host::{Phase, Quick, Slot, Voice};
use ::folderstorm_session::ipc::Screen;
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::Instant;

/// The height of a line's button.
const ROW_BUTTON: Vec2 = Vec2::new(0.0, 22.0);

/// What a quick action's button says in a line.
fn quick_label(quick: Quick) -> &'static str {
    match quick {
        Quick::Switch => tr!("folderstorm-host-switch"),
        Quick::Show => tr!("folderstorm-host-show"),
        Quick::LogIn => tr!("folderstorm-condensed-login"),
        Quick::Wait => tr!("folderstorm-condensed-wait"),
        Quick::Launch => tr!("folderstorm-host-launch"),
        Quick::Restart => tr!("folderstorm-host-restart"),
        Quick::Nothing => tr!("folderstorm-condensed-nothing"),
    }
}

/// Where a character's viewer is, as an icon and how much it matters:
/// 0 fine, 1 on its way or waiting for the user, 2 not there, 3 wrong.
fn state_sign(state: &Slot) -> (&'static str, u8) {
    match state.phase {
        Phase::Stopped | Phase::Ended { crashed: false } => ("player-stop-filled", 2),
        Phase::Ended { crashed: true } => ("alert-triangle-filled", 3),
        Phase::Unmanaged => ("unlink", 2),
        Phase::Starting => ("hourglass-filled", 1),
        Phase::Running if state.closing => ("hourglass-filled", 1),
        Phase::Running => match state.status.as_ref().map(|status| status.screen) {
            Some(Screen::World) => ("world-filled", 0),
            Some(Screen::Login) => ("login-2", 1),
            Some(Screen::Starting | Screen::Loading) | None => ("hourglass-filled", 1),
        },
    }
}

impl Window_ {
    /// From the cards to the lines, and back.
    pub(super) fn condensed_button(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<Act>) {
        let condensed = self.host.desk.condensed();
        let (icon, label, help) = if condensed {
            ("stack-2", tr!("folderstorm-condensed-off"), tr!("folderstorm-condensed-off-help"))
        } else {
            (
                "baseline-density-small",
                tr!("folderstorm-condensed-on"),
                tr!("folderstorm-condensed-on-help"),
            )
        };
        if widgets::action_button(ui, p, icon, label, ActionKind::Flat, ROW_BUTTON)
            .on_hover_text(help)
            .clicked()
        {
            acts.push(Act::Condensed(!condensed));
        }
    }

    /// The characters, one line each.
    pub(super) fn condensed(&mut self, ui: &mut egui::Ui, p: &Palette, now: Instant, quitting: bool, acts: &mut Vec<Act>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("folderstorm-host-heading")).size(14.5).strong().color(p.ink));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.condensed_button(ui, p, acts);
            });
        });
        ui.add_space(4.0);
        let radius = ui.visuals().window_corner_radius;
        let slots: Vec<(usize, Slot)> = self.host.desk.slots().map(|(i, s)| (i, s.clone())).collect();
        // (the room under the lines: what the host asks, when it asks)
        let asking = quitting || self.closing || self.quit_asked || self.host.desk.voice_waits_for().is_some();
        let under = if self.quit_asked {
            96.0
        } else if asking {
            76.0
        } else {
            34.0
        };
        egui::ScrollArea::vertical()
            .id_salt("folderstorm-host-condensed")
            .auto_shrink([false, true])
            .max_height((ui.available_height() - under).max(30.0))
            .show(ui, |ui| {
                if slots.is_empty() {
                    ui.label(RichText::new(tr!("folderstorm-condensed-empty")).color(p.muted));
                }
                ui.spacing_mut().item_spacing.y = 3.0;
                for (slot, state) in &slots {
                    ui.push_id(("folderstorm-host-line", *slot), |ui| {
                        egui::Frame::new()
                            .fill(p.field)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .corner_radius(radius)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.condensed_line(ui, p, *slot, state, now, quitting, acts);
                            });
                    });
                }
            });
        if let Some(waits) = self.host.desk.voice_waits_for() {
            let name = self.name(waits);
            ui.label(RichText::new(tr!("folderstorm-host-voice-waits", name = name)).color(p.amber));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            self.footer(ui, p, now, acts);
        });
    }

    /// One character's line: its colour and name on the left; on the
    /// right its quick action, then what there is to know of it.
    #[allow(clippy::too_many_arguments)]
    fn condensed_line(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, now: Instant, quitting: bool, acts: &mut Vec<Act>) {
        let desk = &self.host.desk;
        let face = faces::face(p, state);
        let active = desk.active() == Some(slot);
        ui.horizontal(|ui| {
            faces::dot(ui, p, face.tint);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                // the quick action: checkpoint 4's, with its own words
                let quick = if quitting { Quick::Nothing } else { desk.quick(slot) };
                let (icon, help) = quick_face(quick, face.name.clone());
                let enabled = quick != Quick::Nothing;
                if widgets::action_button_if(ui, p, icon, quick_label(quick), ActionKind::Flat, ROW_BUTTON, enabled)
                    .on_hover_text(help)
                    .clicked()
                    && enabled
                {
                    acts.push(Act::Comfort(comfort::Act::Quick(slot)));
                }
                // what waits for an answer in its own window
                let waiting = self.host.inbox.count_for(slot);
                if waiting > 0 {
                    let tip = tr!("folderstorm-condensed-waiting", count = waiting);
                    ui.label(RichText::new(super::inbox::count_text(waiting)).strong().color(p.amber))
                        .on_hover_text(&tip);
                    faces::sign(ui, "bell-filled", p.amber, &tip);
                }
                // its unread lines in the shared chat
                let unread = self.host.chat.unread(slot);
                if unread > 0 {
                    let tip = tr!("folderstorm-host-unread", count = unread);
                    ui.label(RichText::new(super::inbox::count_text(unread)).strong().color(p.violet_light))
                        .on_hover_text(&tip);
                    faces::sign(ui, "message-circle-filled", p.violet_light, &tip);
                }
                if state.monitor.is_some() {
                    // (checkpoint 9: its picture is in its own window)
                    faces::sign(ui, "device-desktop-filled", p.muted, tr!("folderstorm-monitor-in-window"));
                }
                match desk.voice(slot) {
                    Voice::Allowed => {
                        faces::sign(ui, "microphone-filled", p.violet_light, tr!("folderstorm-host-voice-here-help"));
                    }
                    Voice::GivingUp => {
                        faces::sign(ui, "microphone-filled", p.amber, tr!("folderstorm-host-voice-giving-up"));
                    }
                    Voice::WaitsFor(_) | Voice::Off => {}
                }
                // what is wrong, if anything: said in full on hover
                let wrong = warnings(desk, slot, state, now);
                if !wrong.is_empty() {
                    let serious = wrong.iter().any(|(_, serious)| *serious);
                    let mut tip: Vec<String> = wrong.into_iter().map(|(text, _)| text).collect();
                    tip.push(tr!("folderstorm-condensed-more-in-cards").to_owned());
                    let colour = if serious { p.danger } else { p.amber };
                    faces::sign(ui, "alert-triangle-filled", colour, &tip.join("\n"));
                }
                let (icon, weight) = state_sign(state);
                let colour = match weight {
                    0 => p.success,
                    1 => p.amber,
                    2 => p.muted,
                    _ => p.danger,
                };
                let status = status_now(desk, slot, state, now);
                faces::sign(ui, icon, colour, &status);
                // a start that takes a while counts up beside its hourglass
                if let Some(seconds) = desk.starting_for(slot, now).map(|d| d.as_secs()).filter(|s| *s > 0) {
                    let text = tr!("folderstorm-host-starting-for", seconds = seconds);
                    ui.label(RichText::new(text).small().color(p.muted)).on_hover_text(&status);
                }
                // the name takes what is left, cut short when it is long
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let colour = if active { p.violet_light } else { p.ink };
                    let text = RichText::new(format!("{} · {}", slot + 1, face.name)).strong().color(colour);
                    let mut tip = vec![faces::real_name(state)];
                    if state.status.as_ref().is_some_and(|s| !s.name.is_empty()) {
                        tip.push(state.profile.shown().to_owned());
                    }
                    if active {
                        tip.push(tr!("folderstorm-host-active").to_owned());
                    }
                    tip.push(status.clone());
                    ui.add(egui::Label::new(text).truncate()).on_hover_text(tip.join("\n"));
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::{heard, host};
    use super::super::panels::Which;
    use super::super::tests::{draw, draw_in, window};
    use super::*;
    use ::folderstorm_session::identity::Colour;
    use ::folderstorm_session::ipc::{AttentionItem, AttentionKind, AttentionList, Conversation, LineKind};
    use ::folderstorm_session::panels::Panel;

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    fn painted(pixels: &[u32]) -> usize {
        let background = pixels[0];
        pixels.iter().filter(|p| **p != background).count()
    }

    fn running(app: &mut Window_, now: Instant) {
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
    }

    #[test]
    fn the_condensed_layout_is_a_choice_that_is_remembered() {
        let now = Instant::now();
        let mut app = window(host());
        assert!(!app.host.desk.condensed());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-condensed-on")) && shows(&drawn, tr!("folderstorm-host-intro")));
        app.apply(vec![Act::Condensed(true)], now);
        assert!(app.host.desk.condensed() && app.host.desk.config().condensed);
        assert!(app.host.unsaved.is_some(), "the choice is to be written");
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "1 · demo-1") && shows(&drawn, "2 · demo-2"), "{drawn:?}");
        assert!(shows(&drawn, tr!("folderstorm-condensed-off")));
        // what the cards alone hold is not there
        assert!(!shows(&drawn, tr!("folderstorm-host-intro")) && !shows(&drawn, tr!("folderstorm-host-economy")));
        assert!(!shows(&drawn, tr!("folderstorm-host-stage")) && !shows(&drawn, tr!("folderstorm-host-add")));
        // the chat is still beside it
        assert!(shows(&drawn, tr!("folderstorm-chat-all")));
        assert!(painted(&pixels) > 5_000);
        // and back
        app.apply(vec![Act::Condensed(false)], now);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-host-intro")) && !shows(&drawn, tr!("folderstorm-condensed-off")));
    }

    #[test]
    fn a_line_says_what_there_is_to_know_and_does_the_quick_action() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.desk.set_colour(1, Colour::Orange);
        assert_eq!(app.host.desk.set_alias(1, "Shop"), Ok(()));
        app.host.desk.set_condensed(true);
        // unread lines and a question for the second character, in a
        // window that does not read them (the characters' page, narrow)
        heard(&mut app.host, 1, Conversation::Nearby, 1, LineKind::Said, "Jane Doe", "hello");
        heard(&mut app.host, 1, Conversation::Nearby, 2, LineKind::Said, "Jane Doe", "anyone?");
        app.host.chat.attention_list(
            1,
            1,
            AttentionList {
                generation: 1,
                items: vec![AttentionItem {
                    id: 7,
                    kind: AttentionKind::Teleport,
                    title: "Jane Doe".into(),
                    text: "Join me".into(),
                }],
                more: 0,
            },
        );
        app.host.sync_inbox(now);
        app.host.desk.set_panel_out(Panel::Inbox, true);
        draw(&mut app, 420, 300);
        let (drawn, pixels) = draw(&mut app, 420, 300);
        assert!(shows(&drawn, "1 · demo-1") && shows(&drawn, "2 · Shop"), "{drawn:?}");
        assert!(!shows(&drawn, "2 · demo-2"), "the alias stands for the name");
        // the counts: two unread lines, one question
        assert!(drawn.iter().any(|t| t == "2") && drawn.iter().any(|t| t == "1"), "{drawn:?}");
        // each line's quick action: the second switches, the first shows
        assert!(shows(&drawn, tr!("folderstorm-host-switch")) && shows(&drawn, tr!("folderstorm-host-show")));
        let tint = faces::tint(&app.palette, Colour::Orange).expect("a colour");
        let orange = u32::from(tint.r()) << 16 | u32::from(tint.g()) << 8 | u32::from(tint.b());
        assert!(pixels.iter().any(|pixel| *pixel & 0x00FF_FFFF == orange), "the character's colour");
        assert!(painted(&pixels) > 1_500);
        // the line's button is the quick action, nothing else
        app.apply(vec![Act::Comfort(comfort::Act::Quick(1))], now);
        assert_eq!(app.host.desk.active(), Some(1));
        assert_eq!(app.host.desk.live(), 2, "never a second process");
        // a viewer that crashed says so, and offers to start again
        app.host.desk.exited(0, 1, false, now);
        let (drawn, _) = draw(&mut app, 420, 300);
        assert!(shows(&drawn, tr!("folderstorm-host-restart")), "{drawn:?}");
        assert_eq!(
            state_sign(app.host.desk.slot(0).expect("a character")),
            ("alert-triangle-filled", 3)
        );
    }

    #[test]
    fn the_condensed_layout_works_with_panels_in_their_own_windows() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.desk.set_condensed(true);
        let pop_out = |app: &mut Window_, panel: Panel| {
            app.host.desk.set_panel_out(panel, true);
            app.panes[panel.index()] = Some(app.look.view());
        };
        // the chat out: the host's window is the lines alone, however small
        pop_out(&mut app, Panel::Chat);
        draw(&mut app, 340, 200);
        let (drawn, pixels) = draw(&mut app, 340, 200);
        assert!(shows(&drawn, "1 · demo-1") && shows(&drawn, "2 · demo-2"), "{drawn:?}");
        assert!(!shows(&drawn, tr!("folderstorm-chat-all")));
        assert!(painted(&pixels) > 800);
        // the characters out: their own window has the lines, with the
        // way back to the cards and to the host's window
        pop_out(&mut app, Panel::Controls);
        draw_in(&mut app, Which::Pane(Panel::Controls), 360, 280);
        let (drawn, pixels) = draw_in(&mut app, Which::Pane(Panel::Controls), 360, 280);
        assert!(
            shows(&drawn, "1 · demo-1") && shows(&drawn, tr!("folderstorm-condensed-off")),
            "{drawn:?}"
        );
        assert!(shows(&drawn, tr!("folderstorm-panel-dock")));
        assert!(painted(&pixels) > 800);
        // closing the host while characters run is asked in its own
        // window, in this layout too
        app.host.desk.set_panel_out(Panel::Controls, false);
        app.panes[Panel::Controls.index()] = None;
        app.closing = true;
        let (drawn, _) = draw(&mut app, 420, 300);
        assert!(shows(&drawn, tr!("folderstorm-host-close-question")), "{drawn:?}");
    }

    #[test]
    fn a_monitor_s_picture_is_in_its_own_window_whatever_the_layout() {
        use super::super::monitor_windows::Seat;
        use ::folderstorm_session::host::Watch;
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.desk.set_monitor(1, Some(Default::default()));
        // neither the cards nor the lines draw a picture: nothing is
        // asked until the monitor's own window shows it
        draw(&mut app, 1120, 1100);
        app.monitors_watch(now);
        assert_eq!(app.host.desk.watch(1), Watch::Hidden);
        app.host.desk.set_condensed(true);
        let (drawn, _) = draw(&mut app, 1120, 1100);
        assert!(shows(&drawn, "2 · demo-2"));
        app.monitors_watch(now);
        assert_eq!(app.host.desk.watch(1), Watch::Hidden);
        // its window draws it: asked, condensed or not
        app.monitors.test_open(Seat::Slot(1), app.look.view());
        draw_in(&mut app, Which::Monitor(1), 480, 300);
        app.monitors_watch(now);
        assert!(matches!(app.host.desk.watch(1), Watch::Asked { .. }));
    }

    #[test]
    fn every_quick_action_has_its_word() {
        for quick in [
            Quick::Switch,
            Quick::Show,
            Quick::LogIn,
            Quick::Wait,
            Quick::Launch,
            Quick::Restart,
            Quick::Nothing,
        ] {
            assert!(!quick_label(quick).is_empty(), "{quick:?}");
        }
    }

    /// Every icon this file names is one the viewer embeds.
    #[test]
    fn the_condensed_layout_s_icons_exist() {
        for name in [
            "stack-2",
            "baseline-density-small",
            "bell-filled",
            "message-circle-filled",
            "device-desktop-filled",
            "microphone-filled",
            "alert-triangle-filled",
            "player-stop-filled",
            "unlink",
            "hourglass-filled",
            "world-filled",
            "login-2",
        ] {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
