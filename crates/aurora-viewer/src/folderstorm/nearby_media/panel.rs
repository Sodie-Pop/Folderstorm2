//! Folderstorm: the "Nearby media" panel under the top bar's media button
//! (LLStatusBar::onMouseEnterNearbyMedia, panel_nearby_media.xml). It opens
//! when the pointer is on the button, like Aurora's volume panel opens on the
//! speaker, stays while the pointer is on the button or the panel, and
//! closes a moment after it left both, or at a click elsewhere.

use super::{Action, Row, Snapshot, Source, State};
use crate::theme::Palette;
use crate::ui::icons::Icons;
use crate::ui::widgets;
use aurora_i18n::tr;
use egui::{RichText, Vec2};

/// How long the panel stays once the pointer left the button and the
/// panel: long enough to travel from the icon down to the panel (the volume
/// panel's delay).
pub(super) const CLOSE_DELAY: f64 = 0.35;

/// Width of the panel's list.
const WIDTH: f32 = 330.0;
/// The list scrolls past this height.
const LIST_HEIGHT: f32 = 240.0;
/// Width of a row's state.
const STATE_WIDTH: f32 = 74.0;
/// A row's buttons (`widgets::icon_button`).
const BUTTON: f32 = 20.0;

/// The panel's open state, between frames (egui's memory).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Hover {
    pub open: bool,
    /// When the pointer left both the button and the open panel.
    left_at: Option<f64>,
}

impl Hover {
    /// One frame: opens as soon as the pointer is on the button, stays
    /// while it is on the button or the panel (or a button is held), closes
    /// `CLOSE_DELAY` after it left both, and at once when the pointer went
    /// to the speaker (its volume panel opens instead). Returns whether a
    /// repaint is needed to close it on time.
    pub fn step(&mut self, on_button: bool, on_panel: bool, held: bool, other: bool, now: f64) -> bool {
        if other && !on_button {
            *self = Hover::default();
            return false;
        }
        if on_button || (self.open && (on_panel || held)) {
            self.open = true;
            self.left_at = None;
            return false;
        }
        if !self.open {
            return false;
        }
        let left = *self.left_at.get_or_insert(now);
        if now - left >= CLOSE_DELAY {
            *self = Hover::default();
            false
        } else {
            true
        }
    }
}

fn hover_id() -> egui::Id {
    egui::Id::unique("fs_nearby_media_hover")
}

/// `AURORA_DEMO_NEARBY_MEDIA=open` pins the panel open (demo captures).
fn demo_pinned() -> bool {
    static PINNED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PINNED.get_or_init(|| std::env::var("AURORA_DEMO_NEARBY_MEDIA").is_ok_and(|v| v == "open"))
}

pub(super) fn is_open(ctx: &egui::Context) -> bool {
    demo_pinned() || ctx.data(|d| d.get_temp::<Hover>(hover_id())).is_some_and(|h| h.open)
}

/// The button's glyph: the media's while nothing plays, a pause while
/// something does (the Legacy Viewer's Camera_Off / Pause_Off faces).
pub fn button_icon(playing: bool) -> &'static str {
    if playing { "player-pause-filled" } else { "brand-youtube-filled" }
}

/// After the top bar drew the media button: opens, keeps or closes the
/// panel and draws it. `other` is the pointer being on the speaker, whose
/// volume panel takes over.
pub fn hover(ui: &mut egui::Ui, p: &Palette, icons: &Icons, button: &egui::Response, other: bool) {
    let ctx = ui.ctx().clone();
    let mut state: Hover = ctx.data(|d| d.get_temp(hover_id())).unwrap_or_default();
    let on_button = button.contains_pointer();
    let pinned = demo_pinned();
    if on_button || pinned {
        state.step(true, false, false, false, 0.0);
    } else if other {
        state.step(false, false, false, true, 0.0);
    }
    if state.open {
        match super::current(&ctx) {
            Some(snapshot) => {
                let anchor = egui::pos2(button.rect.right() + 4.0, button.rect.bottom() + 6.0);
                let area = egui::Area::new(egui::Id::unique("fs_nearby_media_panel"))
                    .fade_in(false)
                    .order(egui::Order::Foreground)
                    .pivot(egui::Align2::RIGHT_TOP)
                    .fixed_pos(anchor)
                    .constrain(true)
                    .show(&ctx, |ui| {
                        widgets::overlay(ui, p, egui::Margin::symmetric(10, 8), |ui| {
                            for action in contents(ui, p, icons, &snapshot) {
                                super::push_action(ui.ctx(), action);
                            }
                        });
                    });
                let on_panel = area.response.contains_pointer();
                let (held, pressed, now) = ui.input(|i| (i.pointer.any_down(), i.pointer.any_pressed(), i.time));
                if !pinned {
                    if pressed && !on_panel && !on_button {
                        // a click elsewhere closes it at once, like the other pop-ups
                        state = Hover::default();
                    } else if state.step(on_button, on_panel, held && !pressed, false, now) {
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(CLOSE_DELAY));
                    }
                }
            }
            // just opened: the app publishes the rows from the next frame
            None => ctx.request_repaint(),
        }
    }
    ctx.data_mut(|d| d.insert_temp(hover_id(), state));
}

/// The panel's body; returns what was clicked.
fn contents(ui: &mut egui::Ui, p: &Palette, icons: &Icons, snapshot: &Snapshot) -> Vec<Action> {
    let mut out = Vec::new();
    ui.set_width(WIDTH);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr!("fs-nearby-media-title")).size(12.5).strong().color(p.ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, p, "settings-filled", tr!("fs-nearby-media-preferences"), true) {
                out.push(Action::OpenPreferences);
            }
        });
    });
    ui.add_space(2.0);
    if !snapshot.media_enabled {
        note(ui, p, tr!("fs-nearby-media-disabled"));
    } else if snapshot.plugins_missing {
        note(ui, p, tr!("fs-nearby-media-no-plugins"));
    }
    if snapshot.rows.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(tr!("fs-nearby-media-empty")).size(12.0).color(p.muted));
        ui.add_space(6.0);
    } else {
        egui::ScrollArea::vertical()
            .id_salt("fs_nearby_media_list")
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for row in &snapshot.rows {
                    row_ui(ui, p, icons, snapshot, row, &mut out);
                }
            });
    }
    ui.add_space(4.0);
    widgets::divider(ui, p);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let size = Vec2::new(0.0, 24.0);
        let stop = widgets::action_button_if(
            ui,
            p,
            "player-stop-filled",
            tr!("fs-nearby-media-stop-all"),
            widgets::ActionKind::Flat,
            size,
            snapshot.can_stop_all(),
        );
        if stop.on_hover_text(tr!("fs-nearby-media-stop-all-tip")).clicked() && snapshot.can_stop_all() {
            out.push(Action::StopAll);
        }
        let start = widgets::action_button_if(
            ui,
            p,
            "player-play-filled",
            tr!("fs-nearby-media-start-all"),
            widgets::ActionKind::Flat,
            size,
            snapshot.can_start_all(),
        );
        if start.on_hover_text(tr!("fs-nearby-media-start-all-tip")).clicked() && snapshot.can_start_all() {
            out.push(Action::StartAll);
        }
    });
    out
}

fn note(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(p.amber)).wrap());
    ui.add_space(2.0);
}

pub(super) fn source_icon(source: Source) -> &'static str {
    match source {
        Source::ParcelMedia => "movie",
        Source::ParcelMusic => "radio",
        Source::Prim { .. } => "box",
    }
}

pub(super) fn state_text(state: State) -> &'static str {
    match state {
        State::Stopped => tr!("fs-nearby-media-state-stopped"),
        State::Waiting => tr!("fs-nearby-media-state-waiting"),
        State::Starting => tr!("fs-nearby-media-state-starting"),
        State::Playing => tr!("fs-nearby-media-state-playing"),
        State::Paused => tr!("fs-nearby-media-state-paused"),
        State::Failed => tr!("fs-nearby-media-state-failed"),
    }
}

fn state_color(p: &Palette, state: State) -> egui::Color32 {
    match state {
        State::Playing => p.teal,
        State::Waiting | State::Starting | State::Paused => p.amber,
        State::Failed => p.danger,
        State::Stopped => p.muted,
    }
}

/// One source: its icon, name (and distance), state, start / stop and mute.
fn row_ui(ui: &mut egui::Ui, p: &Palette, icons: &Icons, snapshot: &Snapshot, row: &Row, out: &mut Vec<Action>) {
    let enabled = snapshot.media_enabled || !row.source.is_media();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
        if let Some(t) = icons.get(source_icon(row.source)) {
            ui.painter().image(
                t.id(),
                egui::Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                p.muted,
            );
        }
        let spacing = ui.spacing().item_spacing.x;
        let name_width = (ui.available_width() - STATE_WIDTH - 2.0 * (BUTTON + spacing) - spacing).max(40.0);
        let name = if matches!(row.source, Source::Prim { .. }) && row.distance >= 1.0 {
            widgets::name_with_suffix(
                &row.name,
                &format!("  {}", tr!("fs-nearby-media-distance", metres = format!("{:.0}", row.distance))),
                egui::FontId::proportional(12.0),
                p.ink,
            )
        } else {
            egui::text::LayoutJob::simple_singleline(row.name.clone(), egui::FontId::proportional(12.0), p.ink)
        };
        let name = ui.allocate_ui(Vec2::new(name_width, 18.0), |ui| {
            ui.set_width(name_width);
            ui.add(widgets::elided_label(ui, name, name_width))
        });
        let tip = [row.name.as_str(), row.url.as_str(), row.note.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .fold(Vec::<&str>::new(), |mut v, s| {
                if !v.contains(s) {
                    v.push(s);
                }
                v
            })
            .join("\n");
        name.inner.on_hover_text(tip);
        ui.allocate_ui(Vec2::new(STATE_WIDTH, 18.0), |ui| {
            ui.set_width(STATE_WIDTH);
            ui.add(widgets::elided_label(
                ui,
                RichText::new(state_text(row.state)).size(11.5).color(state_color(p, row.state)),
                STATE_WIDTH,
            ));
        });
        let (icon, tip) = if row.on {
            ("player-stop-filled", tr!("fs-nearby-media-stop"))
        } else {
            ("player-play-filled", tr!("fs-nearby-media-start"))
        };
        if button(ui, row.source, "toggle", |ui| widgets::icon_button(ui, p, icon, tip, enabled)) {
            out.push(Action::Toggle(row.source));
        }
        if row.source.is_media() {
            let (icon, tip) = if row.muted {
                ("volume-off", tr!("common-unmute"))
            } else {
                ("volume", tr!("common-mute"))
            };
            if button(ui, row.source, "mute", |ui| {
                if enabled {
                    widgets::icon_toggle(ui, p, icon, tip, row.muted)
                } else {
                    widgets::icon_button(ui, p, icon, tip, false)
                }
            }) {
                out.push(Action::Mute(row.source));
            }
        } else {
            ui.add_space(BUTTON);
        }
    });
}

/// A row's button; the frame tests find it by its source and kind.
fn button(ui: &mut egui::Ui, source: Source, kind: &'static str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let r = ui.scope(add);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique(("fs_nearby_media_button", source, kind)), r.response.rect));
    #[cfg(not(test))]
    let _ = (source, kind);
    r.inner
}

/// Where a row's button was drawn (frame tests).
#[cfg(test)]
pub(super) fn button_rect(ctx: &egui::Context, source: Source, kind: &'static str) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp(egui::Id::unique(("fs_nearby_media_button", source, kind))))
}
