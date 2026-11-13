//! The window's side of the host's MCP controller (folderstorm/plans/
//! mcp-routing.md, section 3): its section of the settings window ("Enable
//! controller MCP", "Allow profile and monitor metadata", Ask / Allow /
//! Never for each class, "Allow controller access" for each character), and
//! the Ask window: a small window of its own, in front, centred on the
//! host's window, which shows the oldest question while any waits.
//!
//! Nothing here is said to a viewer, and nothing here touches a viewer's
//! local assistant: every change goes to the desk.

use super::panels::View;
use super::settings::{heading, note, row};
use super::{Act as HostAct, BUTTON, Window_};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::controller::{Action, Settings};
use ::folderstorm_session::ipc::Standby;
use aurora_i18n::tr;
use egui::RichText;
use folderstorm_mcp_bridge::Level;
use folderstorm_mcp_bridge::controller::HostClass;
use std::time::Instant;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// The Ask window's inside, in the screen's points.
const ASK_SIZE: [f32; 2] = [460.0, 250.0];

/// What the window keeps for the controller.
#[derive(Default)]
pub(super) struct Ask {
    /// The Ask window, while a question waits.
    pub view: Option<View>,
    /// The question it was brought forward for.
    raised_for: Option<u64>,
}

/// A click for the controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Act {
    Settings(Settings),
    Access(usize, bool),
    /// The user's answer to question `number`.
    Answer(u64, bool),
}

fn class_name(class: HostClass) -> &'static str {
    match class {
        HostClass::Activate => tr!("folderstorm-controller-class-activate"),
        HostClass::BackgroundMode => tr!("folderstorm-controller-class-background"),
        HostClass::Monitors => tr!("folderstorm-controller-class-monitors"),
        HostClass::OpenLogin => tr!("folderstorm-controller-class-login"),
        HostClass::Restart => tr!("folderstorm-controller-class-restart"),
        HostClass::Logout => tr!("folderstorm-controller-class-logout"),
        HostClass::StopManaging => tr!("folderstorm-controller-class-stop"),
    }
}

impl Window_ {
    pub(super) fn controller_apply(&mut self, act: Act, now: Instant) {
        match act {
            Act::Settings(settings) => self.host.desk.set_controller(settings),
            Act::Access(slot, allowed) => self.host.desk.set_controller_access(slot, allowed),
            Act::Answer(number, allow) => self.host.controller_decide(number, allow, now),
        }
    }

    /// The settings window's "MCP controller" section.
    pub(super) fn controller_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let settings = self.host.desk.controller();
        let mut edited = settings;
        heading(ui, p, tr!("folderstorm-controller-access"));
        widgets::checkbox(ui, p, &mut edited.enabled, tr!("folderstorm-controller-enable"))
            .on_hover_text(tr!("folderstorm-controller-enable-help"));
        widgets::checkbox(ui, p, &mut edited.metadata, tr!("folderstorm-controller-metadata"))
            .on_hover_text(tr!("folderstorm-controller-metadata-help"));
        let status = match (settings.enabled, self.host.controller.port(), &self.host.controller.failure) {
            (false, _, _) => tr!("folderstorm-controller-off").to_owned(),
            (true, _, Some(failure)) => tr!("folderstorm-controller-failed", reason = failure.clone()),
            (true, Some(port), None) => tr!("folderstorm-controller-listening", port = port),
            (true, None, None) => tr!("folderstorm-controller-starting").to_owned(),
        };
        note(ui, p, status);
        note(ui, p, tr!("folderstorm-controller-separate"));
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-controller-actions"));
        note(ui, p, tr!("folderstorm-controller-actions-intro"));
        for class in HostClass::ALL {
            ui.push_id(("folderstorm-controller-class", class as u8), |ui| {
                row(ui, p, class_name(class), |ui| {
                    let levels = [Level::Ask, Level::Allow, Level::Never];
                    let mut index = levels.iter().position(|l| *l == edited.levels.level(class)).unwrap_or(0);
                    let names = [
                        (tr!("folderstorm-controller-ask"), true),
                        (tr!("folderstorm-controller-allow"), true),
                        (tr!("folderstorm-controller-never"), true),
                    ];
                    if widgets::segmented(ui, p, &mut index, &names) {
                        edited.levels.set(class, levels[index.min(2)]);
                    }
                });
            });
        }
        if edited != settings {
            acts.push(HostAct::Controller(Act::Settings(edited)));
        }
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-controller-characters"));
        note(ui, p, tr!("folderstorm-controller-characters-intro"));
        let slots: Vec<_> = self.host.desk.slots().map(|(slot, s)| (slot, s.clone())).collect();
        if slots.is_empty() {
            note(ui, p, tr!("folderstorm-host-empty"));
        }
        for (slot, state) in slots {
            ui.push_id(("folderstorm-controller-access", slot), |ui| {
                let mut allowed = state.controller;
                let label = tr!(
                    "folderstorm-controller-allow-character",
                    number = slot + 1,
                    name = super::shown_name(&state)
                );
                if widgets::checkbox(ui, p, &mut allowed, label).changed() {
                    acts.push(HostAct::Controller(Act::Access(slot, allowed)));
                }
            });
        }
    }

    // ---- the Ask window ----

    /// What a question asks, in plain words.
    fn question_sentence(&self, slot: Option<usize>, action: Action) -> String {
        let name = slot
            .and_then(|slot| self.host.desk.slot(slot).map(|s| (slot, super::shown_name(s))))
            .map(|(slot, name)| (slot + 1, name));
        let (number, name) = name.unwrap_or((0, String::new()));
        match action {
            Action::Activate => tr!("folderstorm-controller-ask-activate", name = name, number = number),
            Action::Background(Standby::Warm) => tr!("folderstorm-controller-ask-warm", name = name, number = number),
            Action::Background(Standby::Economy) => tr!("folderstorm-controller-ask-economy", name = name, number = number),
            Action::Monitor(true) => tr!("folderstorm-controller-ask-monitor-open", name = name, number = number),
            Action::Monitor(false) => tr!("folderstorm-controller-ask-monitor-close", name = name, number = number),
            Action::Cycling(true) => tr!("folderstorm-controller-ask-cycling-open").to_owned(),
            Action::Cycling(false) => tr!("folderstorm-controller-ask-cycling-close").to_owned(),
            Action::OpenLogin => tr!("folderstorm-controller-ask-login", name = name, number = number),
            Action::Restart => tr!("folderstorm-controller-ask-restart", name = name, number = number),
            Action::Logout => tr!("folderstorm-controller-ask-logout", name = name, number = number),
            Action::StopManaging => tr!("folderstorm-controller-ask-stop", name = name, number = number),
            Action::List => String::new(),
        }
    }

    /// The Ask window's content: the oldest question.
    pub(super) fn ask_window(&mut self, ui: &mut egui::Ui, acts: &mut Vec<HostAct>) {
        let p = self.palette;
        let now = Instant::now();
        let Some(q) = self.host.controller.questions.front() else {
            return;
        };
        let (number, slot, action, client) = (q.number, q.slot, q.request.action, q.client.clone());
        let left = q.seconds_left(now);
        let waiting = self.host.controller.questions.len().saturating_sub(1);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.panel).inner_margin(16.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!("folderstorm-controller-ask-title"))
                        .strong()
                        .size(16.0)
                        .color(p.ink),
                );
                ui.add_space(6.0);
                let who = match &client {
                    Some(client) => tr!("fs-mcp-ask-who", client = client.clone()),
                    None => tr!("fs-mcp-ask-who-unnamed").to_owned(),
                };
                ui.label(RichText::new(who).color(p.ink)).on_hover_text(tr!("fs-mcp-ask-who-hint"));
                ui.add_space(4.0);
                ui.label(RichText::new(self.question_sentence(slot, action)).strong().color(p.ink));
                ui.add_space(6.0);
                ui.label(RichText::new(tr!("fs-mcp-ask-countdown", seconds = left)).size(12.0).color(p.muted));
                if waiting > 0 {
                    ui.label(RichText::new(tr!("fs-mcp-ask-more", count = waiting)).size(12.0).color(p.muted));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    // neither is the default: an Enter typed for something
                    // else allows nothing
                    let size = egui::Vec2::new(120.0, BUTTON.y);
                    if widgets::action_button(ui, &p, "check", tr!("fs-mcp-ask-allow"), ActionKind::Flat, size).clicked() {
                        acts.push(HostAct::Controller(Act::Answer(number, true)));
                    }
                    if widgets::action_button(ui, &p, "x", tr!("fs-mcp-ask-refuse"), ActionKind::Flat, size).clicked() {
                        acts.push(HostAct::Controller(Act::Answer(number, false)));
                    }
                });
                note(ui, &p, tr!("folderstorm-controller-ask-note"));
            });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            acts.push(HostAct::Controller(Act::Answer(number, false)));
        }
    }

    /// The Ask window opens while a question waits, comes forward for
    /// each new one, and closes when none is left.
    pub(super) fn sync_ask(&mut self, event_loop: &ActiveEventLoop) {
        let front = self.host.controller.questions.front().map(|q| q.number);
        match (front, self.ask.view.is_some()) {
            (Some(_), false) => match self.open_ask_window(event_loop) {
                Ok(view) => {
                    self.ask.view = Some(view);
                    self.ask.raised_for = None;
                    self.redraw_all();
                }
                Err(e) => {
                    // no window: the questions are refused rather than
                    // waiting unseen
                    log::warn!("session host: no window for an MCP controller question ({e}): refused");
                    if let Some(number) = front {
                        self.host.controller_decide(number, false, Instant::now());
                    }
                }
            },
            (None, true) => {
                if let Some(mut view) = self.ask.view.take() {
                    view.detach();
                }
                self.ask.raised_for = None;
            }
            _ => {}
        }
        if let Some(number) = front
            && self.ask.raised_for != Some(number)
            && let Some(window) = self.ask.view.as_ref().and_then(|view| view.window.as_ref())
        {
            self.ask.raised_for = Some(number);
            window.set_minimized(false);
            crate::folderstorm_characters::foreground::bring_forward(window);
        }
    }

    fn open_ask_window(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<View> {
        use winit::dpi::{LogicalSize, PhysicalPosition};
        let mut attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(format!("{} — {}", tr!("folderstorm-controller-ask-title"), self.title))
            .with_inner_size(LogicalSize::new(ASK_SIZE[0], ASK_SIZE[1]))
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop);
        // centred on the host's window (its last place while it hides
        // behind a docked bar)
        if let Some(main) = &self.main.window
            && let Ok(at) = main.outer_position()
        {
            let outer = main.outer_size();
            let scale = main.scale_factor();
            let width = (f64::from(ASK_SIZE[0]) * scale) as i32;
            let height = (f64::from(ASK_SIZE[1]) * scale) as i32;
            let x = at.x + (outer.width as i32 - width) / 2;
            let y = at.y + (outer.height as i32 - height) / 2;
            attributes = attributes.with_position(PhysicalPosition::new(x, y));
        }
        let mut view = self.look.view();
        view.attach(event_loop, event_loop.create_window(attributes)?)?;
        Ok(view)
    }

    /// What the Ask window is told before its interface hears it. True
    /// when that was all (closed: the question shown is refused).
    pub(super) fn ask_event(&mut self, event: &WindowEvent) -> bool {
        if matches!(event, WindowEvent::CloseRequested) {
            if let Some(number) = self.host.controller.questions.front().map(|q| q.number) {
                self.host.controller_decide(number, false, Instant::now());
            }
            if self.host.controller.questions.is_empty()
                && let Some(mut view) = self.ask.view.take()
            {
                view.detach();
            }
            self.redraw_all();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::super::panels::Which;
    use super::super::tests::draw_in;
    use super::*;
    use folderstorm_mcp_bridge::Pending;
    use serde_json::json;

    fn window() -> Window_ {
        super::super::tests::window(super::super::super::chat_ui::tests::host())
    }

    #[test]
    fn the_section_shows_every_class_and_character_and_its_clicks_go_to_the_desk() {
        let mut app = window();
        app.settings_apply(super::super::settings::Act::Open(
            ::folderstorm_session::settings::Section::Controller,
            None,
        ));
        draw_in(&mut app, Which::Settings, 1000, 900);
        let (drawn, pixels) = draw_in(&mut app, Which::Settings, 1000, 900);
        assert!(pixels.iter().any(|p| *p != pixels[0]));
        let text = drawn.join("\n");
        for class in HostClass::ALL {
            assert!(text.contains(class_name(class)), "{class:?}");
        }
        assert!(text.contains(tr!("folderstorm-controller-enable")));
        assert!(text.contains(tr!("folderstorm-controller-off")));
        // the clicks
        let now = Instant::now();
        let mut s = app.host.desk.controller();
        s.enabled = true;
        app.apply(
            vec![HostAct::Controller(Act::Settings(s)), HostAct::Controller(Act::Access(1, true))],
            now,
        );
        assert!(app.host.desk.controller().enabled);
        assert!(app.host.desk.slot(1).is_some_and(|s| s.controller));
        assert!(app.host.controller.port().is_some(), "listening once turned on");
        let (drawn, _) = draw_in(&mut app, Which::Settings, 1000, 900);
        let port = app.host.controller.port().expect("port").to_string();
        assert!(drawn.join("\n").contains(&port));
        // turned off: the listener stops
        s.enabled = false;
        app.apply(vec![HostAct::Controller(Act::Settings(s))], now);
        assert!(app.host.controller.port().is_none());
    }

    #[test]
    fn a_question_shows_its_sentence_and_its_answer_is_carried_out() {
        let now = Instant::now();
        let mut app = window();
        app.host.desk.set_controller(Settings {
            enabled: true,
            metadata: false,
            levels: folderstorm_mcp_bridge::controller::HostLevels::all(Level::Ask),
        });
        app.host.desk.set_controller_access(1, true);
        app.host.tick(now);
        let (call, answer) = Pending::detached(
            "host_background_mode",
            json!({"profile": "demo-2", "mode": "economy"}),
            true,
            Some("Claude Code"),
        );
        let request = ::folderstorm_session::controller::parse(&call.tool, &call.arguments).expect("parsed");
        app.host
            .controller
            .questions
            .ask(&app.host.desk, Some(1), request, Some("Claude Code".into()), call, now);
        let (drawn, _) = draw_in(&mut app, Which::Ask, 460, 250);
        let text = drawn.join("\n");
        assert!(text.contains("Claude Code"), "{text}");
        assert!(text.contains(&app.question_sentence(Some(1), Action::Background(Standby::Economy))));
        assert!(text.contains(tr!("fs-mcp-ask-allow")) && text.contains(tr!("fs-mcp-ask-refuse")));
        let number = app.host.controller.questions.front().expect("asked").number;
        app.apply(vec![HostAct::Controller(Act::Answer(number, true))], now);
        assert_eq!(app.host.desk.slot(1).expect("demo-2").standby, Standby::Economy);
        assert_eq!(answer.try_recv().expect("answered").expect("done")["done"], true);
        assert!(app.host.controller.questions.is_empty());
        // a second answer to it does nothing
        app.apply(vec![HostAct::Controller(Act::Answer(number, false))], now);
        // refused: the call is refused, nothing changes
        let (call, answer) = Pending::detached("host_background_mode", json!({"profile": "demo-2", "mode": "warm"}), true, None);
        let request = ::folderstorm_session::controller::parse(&call.tool, &call.arguments).expect("parsed");
        let number = app.host.controller.questions.ask(&app.host.desk, Some(1), request, None, call, now);
        let (drawn, _) = draw_in(&mut app, Which::Ask, 460, 250);
        assert!(drawn.join("\n").contains(tr!("fs-mcp-ask-who-unnamed")));
        assert!(app.ask_event(&WindowEvent::CloseRequested), "closing the window refuses");
        assert_eq!(
            answer.try_recv().expect("answered").expect_err("refused").code,
            folderstorm_mcp_bridge::ErrorCode::PermissionDenied
        );
        assert_eq!(app.host.desk.slot(1).expect("demo-2").standby, Standby::Economy);
        assert!(app.host.controller.questions.take(number).is_none());
    }
}
