//! Folderstorm: the local assistant's Ask dialog (folderstorm/plans/mcp.md,
//! "Ask"). One request at a time, oldest first: which program asks, what
//! it asks in plain words, Allow or Refuse. Escape, a click outside and the
//! end of the 60 seconds refuse. Neither button is the dialog's default:
//! Enter, typed for something else when the dialog opens, allows nothing.

use crate::theme::Palette;
use crate::ui::widgets;
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::{Duration, Instant};

/// Draw the dialog of the oldest open request, if any.
pub fn show(ctx: &egui::Context, p: &Palette) {
    let Some(question) = super::with(|rt| rt.desk.question(Instant::now())) else {
        return;
    };
    let mut answer = None;
    let modal = widgets::Modal::new(egui::Id::unique(("fs_mcp_ask", question.number))).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(RichText::new(tr!("fs-mcp-ask-title")).strong().color(p.ink));
        ui.add_space(6.0);
        let who = match &question.client {
            Some(client) => tr!("fs-mcp-ask-who", client = client.clone()),
            None => tr!("fs-mcp-ask-who-unnamed").to_owned(),
        };
        ui.label(RichText::new(who).color(p.ink)).on_hover_text(tr!("fs-mcp-ask-who-hint"));
        ui.add_space(4.0);
        ui.label(RichText::new(&question.summary).strong().color(p.ink));
        // Folderstorm: a plan's rows (mcp-receipts.md, section 6)
        for line in &question.detail {
            ui.label(RichText::new(line).size(12.5).color(p.ink));
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(tr!("fs-mcp-ask-countdown", seconds = question.seconds_left))
                .size(12.0)
                .color(p.muted),
        );
        if question.waiting > 0 {
            ui.label(
                RichText::new(tr!("fs-mcp-ask-more", count = question.waiting))
                    .size(12.0)
                    .color(p.muted),
            );
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let size = Vec2::new(110.0, 26.0);
            if widgets::action_button(ui, p, "check", tr!("fs-mcp-ask-allow"), widgets::ActionKind::Flat, size).clicked() {
                answer = Some(true);
            }
            if widgets::action_button(ui, p, "x", tr!("fs-mcp-ask-refuse"), widgets::ActionKind::Flat, size).clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && modal.should_close() {
        answer = Some(false);
    }
    if let Some(allow) = answer {
        super::with(|rt| rt.desk.decide(question.number, allow));
    }
    // the countdown, and the refusal at its end
    ctx.request_repaint_after(Duration::from_millis(250));
}
