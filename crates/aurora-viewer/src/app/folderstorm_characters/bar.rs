//! Folderstorm: a managed viewer's side of the session host's bar
//! (session-host plan, checkpoint 8). The host says what the bar shows
//! (`ToViewer::Bar`); this viewer draws it in a strip at the top of its
//! window and sends each click back (`ToHost::Bar`). It decides nothing:
//! the drawing is `crate::folderstorm_characters::bar`, the rules the
//! `folderstorm-session` crate's.
//!
//! - **The strip is egui's "safe area".** Before the interface's pass the
//!   strip's height is given as the top inset of the input
//!   ([`App::folderstorm_bar_inset`], a hook in `App::frame`): egui then
//!   takes it out of `content_rect()`, which the login and loading
//!   screens, the windows and the notices lay themselves out in, so they
//!   all start under the bar without a line of theirs being changed. The
//!   viewer's own bars are laid out in egui's root `Ui`, which spans the
//!   window whatever the inset: a second hook, where the custom world
//!   viewport already gives them their room, puts them under the strip
//!   (`bar::bars_room`). The bar is drawn in the strip, above them all.
//! - **Nothing without a host.** A viewer that no host started never
//!   touches the input. One that lost its host has no bar's state any
//!   more: the inset is zero and its interface has the whole window.
//! - **Hidden with the interface**: no bar while the interface is hidden,
//!   in mouselook or in the full-frame photo view; the host's hotkeys
//!   still switch.
//! - **What the bar's state holds stays here.** The names, aliases and
//!   colours of the other characters are read by the drawing and by
//!   nothing else: no log line, no setting, no chat line, nothing sent to
//!   a grid.
//!
//! The window's place on the taskbar ("one taskbar button") is here too:
//! [`App::folderstorm_taskbar`], from the event loop's turn.

use super::*;
use crate::folderstorm_characters::bar as drawn;
use crate::folderstorm_characters::{managed, taskbar};
use ::folderstorm_session::bar;

impl App {
    /// The bar this frame draws, if any.
    fn folderstorm_bar_state(&self) -> Option<&bar::State> {
        let state = &self.folderstorm_characters;
        let hidden =
            self.interface.hidden || (matches!(self.screen, Screen::World) && (self.camera.mouselook() || self.photo_full_frame()));
        state.bar.as_ref().filter(|bar| state.managed.linked() && bar.docked && !hidden)
    }

    /// Before the interface's pass: the bar's strip is kept out of the
    /// room the interface lays itself out in. Nothing in a viewer that no
    /// host started.
    pub(in crate::app) fn folderstorm_bar_inset(&self, raw: &mut egui::RawInput) {
        if managed::link().is_none() {
            return;
        }
        // (always said: egui keeps the last inset it was given)
        raw.safe_area_insets = Some(drawn::inset(self.folderstorm_bar_state()));
    }

    /// Draws the bar in its strip and sends its clicks to the host.
    pub(super) fn folderstorm_bar(&self, ctx: &egui::Context, p: &crate::theme::Palette) {
        let (Some(state), Some(link)) = (self.folderstorm_bar_state(), managed::link()) else {
            return;
        };
        for request in drawn::show_strip(ctx, p, state) {
            if !link.request(request) {
                log::warn!("session host: a click on the bar was not sent ({request:?})");
            }
        }
    }

    /// Every turn of the event loop: this window on the taskbar and in the
    /// Alt-Tab list, as the host asks of a background character and as any
    /// window otherwise (`bar::taskbar`). Before the window is brought
    /// forward or minimised by the same turn.
    pub(super) fn folderstorm_taskbar(&mut self, now: Instant) {
        let focused = self.folderstorm_focused();
        let Some(g) = &self.gfx else {
            return;
        };
        let state = &mut self.folderstorm_characters;
        let wanted = bar::taskbar(bar::WindowFacts {
            linked: state.managed.linked(),
            asked: state.managed.off_taskbar(),
            active: state.managed.role() == Some(ipc::Role::Active),
            focused,
            minimised: g.window.is_minimized().unwrap_or(false),
            economy: state.managed.standby() == Some(ipc::Standby::Economy),
        });
        taskbar::keep(&g.window, &mut state.taskbar, wanted, now);
    }
}
