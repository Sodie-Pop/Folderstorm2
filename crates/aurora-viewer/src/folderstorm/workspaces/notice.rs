//! The card shown over the world when the monitors changed
//! (folderstorm/plans/workspaces.md section 8.5; checkpoint 4 in detail,
//! point 6): "Your monitors changed. Use the layout “1 monitor”?" with
//! Apply and Not now; which layout was applied when the user chose not to
//! be asked; or that the viewer window was unlocked.
//!
//! Not a dialog: it sits at the top of the room the windows move in, takes
//! neither the keyboard nor the clicks beside it, and answers through the
//! menu's requests (`super::menu`), run before the next frame.

use super::apply::{self, Notice};
use super::menu::{self, Request};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

const ICON: &str = "device-desktop-filled";
const ICON_APPLY: &str = "check";
const ICON_LATER: &str = "x";
const BUTTON_H: f32 = 26.0;
/// The card is at most this wide, in points.
const WIDTH: f32 = 420.0;

/// The card's sentence.
pub(super) fn text(notice: &Notice) -> String {
    match notice {
        Notice::Offer { name, unlocked: false } => tr!("fs-layouts-monitors-offer", name = name.as_str()),
        Notice::Offer { name, unlocked: true } => tr!("fs-layouts-monitors-offer-unlocked", name = name.as_str()),
        Notice::Applied { name } => tr!("fs-layouts-monitors-applied", name = name.as_str()),
        Notice::Unlocked => tr!("fs-layouts-monitors-unlocked").to_owned(),
    }
}

/// Draws the card when there is one (the hook of the interface's pass).
pub fn show(ctx: &egui::Context, p: &Palette) {
    let Some(notice) = apply::notice(ctx) else {
        return;
    };
    let room = crate::ui::layout::work_area(ctx);
    let width = (room.width() - 32.0).clamp(200.0, WIDTH);
    let mut request = None;
    egui::Area::new(egui::Id::unique("folderstorm-layouts-notice"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(room.center_top() + vec2(0.0, 12.0))
        .show(ctx, |ui| {
            widgets::overlay(ui, p, egui::Margin::symmetric(14, 10), |ui| {
                ui.set_max_width(width);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.horizontal(|ui| {
                    if let Some(icon) = crate::ui::icons::global(ICON) {
                        ui.add(egui::Image::new(&icon).tint(p.violet_light).fit_to_exact_size(vec2(18.0, 18.0)));
                    }
                    ui.add(egui::Label::new(egui::RichText::new(text(&notice)).size(13.0).color(p.ink)).wrap());
                });
                ui.horizontal(|ui| {
                    let size = vec2(0.0, BUTTON_H);
                    match &notice {
                        Notice::Offer { name, .. } => {
                            if widgets::action_button(ui, p, ICON_APPLY, tr!("fs-layouts-apply"), ActionKind::Primary, size).clicked() {
                                request = Some(Request::Apply(name.clone()));
                            }
                            if widgets::action_button(ui, p, ICON_LATER, tr!("fs-layouts-monitors-later"), ActionKind::Flat, size).clicked() {
                                request = Some(Request::Dismiss);
                            }
                        }
                        Notice::Applied { .. } => {
                            if widgets::action_button(ui, p, menu::ICON_PREVIOUS, tr!("fs-layouts-previous-button"), ActionKind::Flat, size).clicked() {
                                request = Some(Request::Previous);
                            }
                            if widgets::action_button(ui, p, ICON_APPLY, tr!("common-ok"), ActionKind::Primary, size).clicked() {
                                request = Some(Request::Dismiss);
                            }
                        }
                        Notice::Unlocked => {
                            if widgets::action_button(ui, p, ICON_APPLY, tr!("common-ok"), ActionKind::Primary, size).clicked() {
                                request = Some(Request::Dismiss);
                            }
                        }
                    }
                });
            });
        });
    if let Some(request) = request {
        menu::ask(ctx, request);
    }
}
