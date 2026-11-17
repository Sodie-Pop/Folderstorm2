//! Folderstorm: the margins' desktop in the app (the rules are in
//! `crate::folderstorm::desktop`; folderstorm/plans/margin-desktop.md).
//!
//! Gives the desktops what the app knows this frame (what greys a toolbar
//! button, whether the interface is hidden), and runs what a shortcut asks
//! through Aurora's own path: a command through `ui::bars::run_command`,
//! its action handed to the app in the frame's one slot (`UiActions::bar`),
//! exactly as a toolbar button does, so every check the app makes on that
//! action (RLVa, the region's rights) is made here too.

use super::*;
use crate::folderstorm::desktop::{self, Facts, Request, targets};

/// The desktop changed and is not saved yet.
fn dirty_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_dirty")
}

/// Runs the command `id` as its toolbar button does, behind the button's
/// gates (`targets::command_gate`, checked again here: what greys it may
/// have changed since the shortcut was drawn). The action for the app.
fn run(ctx: &egui::Context, id: &str, facts: &Facts, panels: &mut ui::Panels) -> BarAction {
    if targets::command_gate(id, facts).is_err() {
        return BarAction::None;
    }
    ui::bars::run_command(ctx, id, panels)
}

impl App {
    /// In the interface's pass, once the bars are laid out: the desktops
    /// of the margins, and what their shortcuts ask.
    pub(super) fn folderstorm_desktop(&mut self, ctx: &egui::Context, p: &crate::theme::Palette, a: &mut UiActions) {
        self.desktop_demo(ctx);
        if !self.settings.folderstorm_desktop.shortcuts_on {
            return;
        }
        let facts = self.desktop_facts(ctx);
        // hidden with the interface, and in mouselook as the bars are
        let mouselook = self.camera.mouselook() && !self.settings.camera.show_interface_in_mouselook;
        let env = desktop::Env {
            p,
            icons: &self.skin.icons,
            facts,
            account: self.world.agent_id,
            hidden: mouselook || self.interface.hidden || self.interface.immersive || ui::widgets::interface_hidden(ctx),
        };
        let mut requests = desktop::show(ctx, &mut self.settings.folderstorm_desktop, &env);
        if self.demo
            && self.frame_count == desktop::demo::OPEN_FRAME
            && std::env::var("AURORA_DEMO_DESKTOP").is_ok_and(|mode| mode == "open")
        {
            requests.push(Request::Run(desktop::demo::OPENED.to_owned()));
        }
        for request in requests {
            match request {
                Request::Run(command) => {
                    let action = run(ctx, &command, &facts, &mut self.panels);
                    if !matches!(action, BarAction::None) {
                        a.bar = action;
                    }
                }
                Request::Changed => {
                    ctx.data_mut(|d| d.insert_temp(dirty_id(), true));
                }
                Request::Settings => {
                    self.panels.settings = true;
                    self.options_ui.open(ui::options::Page::Layout, 0);
                    self.options_ui.reveal = Some((ui::options::Page::Layout, 0, aurora_i18n::tr!("fs-desktop-shortcuts")));
                }
            }
        }
        // saved once the pointer is up, as the gesture boards are
        let dirty = ctx.data(|d| d.get_temp::<bool>(dirty_id())).unwrap_or(false);
        if dirty && !ctx.input(|i| i.pointer.any_down()) {
            ctx.data_mut(|d| d.insert_temp(dirty_id(), false));
            self.settings.save();
        }
    }

    /// Whether the pointer is over a desktop (the hook of
    /// `ui_takes_pointer`): a press, a release or the wheel there is the
    /// interface's, never the world's.
    pub(super) fn folderstorm_desktop_takes_pointer(&self) -> bool {
        let ppp = self.egui_ctx.pixels_per_point();
        desktop::takes_pointer(&self.egui_ctx, egui::pos2(self.cursor_pos.0 / ppp, self.cursor_pos.1 / ppp))
    }

    /// What greys a toolbar button this frame: the facts the toolbar's row
    /// reads (`ui::bars`), from where the app puts them.
    fn desktop_facts(&self, ctx: &egui::Context) -> Facts {
        use crate::folderstorm::workspaces::toolbar;
        Facts {
            photo_roll_empty: self.toolbar_ui.photo_roll_empty,
            no_guide: !self.toolbar_ui.destinations,
            // (set each frame by `draw_ui`, RlvActions::canStand / canGroundSit)
            sit_locked: ctx.data(|d| d.get_temp::<bool>(ui::bars::rlva_sit_locked_id())).unwrap_or(false),
            toggle_unset: !toolbar::look(ctx, toolbar::TOGGLE, false, true, String::new()).1,
        }
    }

    /// `AURORA_DEMO_DESKTOP=<mode>`: a desktop made once, at the start of a
    /// demo run (in the demo's own settings), the viewport turned on when
    /// it is off (`crate::folderstorm::desktop::demo`).
    fn desktop_demo(&mut self, ctx: &egui::Context) {
        static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.demo || SEEDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Ok(mode) = std::env::var("AURORA_DEMO_DESKTOP") else {
            return;
        };
        let width = (ctx.content_rect().width() * ctx.pixels_per_point()).round() as u32;
        let s = &mut self.settings;
        if !desktop::demo::seed(&mode, &mut s.folderstorm_desktop, &mut s.folderstorm_viewport, width) {
            log::warn!("AURORA_DEMO_DESKTOP={mode}: not a mode of the demo");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels() -> ui::Panels {
        ui::Panels::from_settings(&crate::settings::Settings::default())
    }

    #[test]
    fn a_command_shortcut_runs_what_its_toolbar_button_runs() {
        let ctx = egui::Context::default();
        let free = Facts::default();
        // a window command toggles its panel, as the button does
        let mut p = panels();
        let was = p.gestures;
        assert!(matches!(run(&ctx, "gestures", &free, &mut p), BarAction::None));
        assert_eq!(p.gestures, !was);
        assert!(matches!(run(&ctx, "gestures", &free, &mut p), BarAction::None));
        assert_eq!(p.gestures, was, "open closes");
        // a command of the app is handed over as an action
        assert!(matches!(run(&ctx, "home", &free, &mut p), BarAction::TeleportHome));
        assert!(matches!(run(&ctx, "sit", &free, &mut p), BarAction::SitGround));
        assert!(matches!(run(&ctx, "stop_animations", &free, &mut p), BarAction::StopAnimations));
    }

    #[test]
    fn a_command_stops_at_its_button_s_gates() {
        let ctx = egui::Context::default();
        let mut p = panels();
        // Sit while RLVa would refuse it
        let locked = Facts {
            sit_locked: true,
            ..Default::default()
        };
        assert!(matches!(run(&ctx, "sit", &locked, &mut p), BarAction::None));
        // nothing to show
        let empty = Facts {
            photo_roll_empty: true,
            no_guide: true,
            ..Default::default()
        };
        assert!(matches!(run(&ctx, "photo_roll", &empty, &mut p), BarAction::None));
        let was = p.destinations;
        assert!(matches!(run(&ctx, "destinations", &empty, &mut p), BarAction::None));
        assert_eq!(p.destinations, was);
        assert!(matches!(run(&ctx, "photo_roll", &Facts::default(), &mut p), BarAction::PhotoRoll));
        // a button that is its own control, a command not in Aurora yet,
        // one the catalogue does not have
        for id in ["favorites", "vr_view", "speak", "flickr", "no-such-command"] {
            assert!(matches!(run(&ctx, id, &Facts::default(), &mut p), BarAction::None), "{id}");
        }
    }
}
