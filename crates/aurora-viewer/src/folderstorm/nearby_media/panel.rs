//! Folderstorm: the "Nearby media" panel under the top bar's media button
//! (LLStatusBar::onMouseEnterNearbyMedia, panel_nearby_media.xml). It opens
//! when the pointer is on the button, like Aurora's volume panel opens on the
//! speaker, stays while the pointer is on the button or the panel, and
//! closes a moment after it left both, or at a click elsewhere. Its "Show"
//! list stays open over it as part of it.

use super::{Action, Filter, Row, Snapshot, Source, State};
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
const WIDTH: f32 = 430.0;
/// The list scrolls past this height.
const LIST_HEIGHT: f32 = 240.0;
/// Width of a row's state.
const STATE_WIDTH: f32 = 74.0;
/// A row's buttons (`widgets::icon_button`).
const BUTTON: f32 = 20.0;
/// A row's volume slider.
const SLIDER: f32 = 64.0;
/// Width of the Show list.
const FILTER_WIDTH: f32 = 150.0;

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

fn filter_id() -> egui::Id {
    egui::Id::unique("fs_nearby_media_filter")
}

/// The Show list's choice: kept while the viewer runs, not saved, as in the
/// Legacy Viewer.
pub(super) fn filter(ctx: &egui::Context) -> Filter {
    ctx.data(|d| d.get_temp(filter_id())).unwrap_or_default()
}

fn set_filter(ctx: &egui::Context, filter: Filter) {
    ctx.data_mut(|d| d.insert_temp(filter_id(), filter));
}

/// Where the Show list's widget id is kept, to know whether its list is
/// open.
fn combo_id_key() -> egui::Id {
    egui::Id::unique("fs_nearby_media_filter_combo")
}

/// The Show list's own list is open: the pointer on it is on the panel.
fn combo_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<egui::Id>(combo_id_key()))
        .is_some_and(|id| egui::ComboBox::is_open(ctx, id))
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
                // a click in the Show list's own list is a click on the panel
                let list_was_open = combo_open(&ctx);
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
                let on_panel = area.response.contains_pointer() || list_was_open || combo_open(&ctx);
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
    let mut shown = filter(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr!("fs-nearby-media-title")).size(12.5).strong().color(p.ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, p, "settings-filled", tr!("fs-nearby-media-preferences"), true) {
                out.push(Action::OpenPreferences);
            }
            if let Some(chosen) = filter_combo(ui, p, shown) {
                shown = chosen;
                set_filter(ui.ctx(), chosen);
            }
            ui.label(RichText::new(tr!("fs-nearby-media-show")).size(12.0).color(p.muted));
        });
    });
    ui.add_space(2.0);
    if !snapshot.media_enabled {
        note(ui, p, tr!("fs-nearby-media-disabled"));
    } else if snapshot.plugins_missing {
        note(ui, p, tr!("fs-nearby-media-no-plugins"));
    }
    if shown == Filter::OnOthers && !snapshot.show_on_others {
        note(ui, p, tr!("fs-nearby-media-others-off"));
    }
    let rows: Vec<&Row> = snapshot.rows.iter().filter(|r| shown.shows(r)).collect();
    if rows.is_empty() {
        let empty = if snapshot.rows.is_empty() {
            tr!("fs-nearby-media-empty")
        } else {
            tr!("fs-nearby-media-empty-filter")
        };
        ui.add_space(6.0);
        ui.label(RichText::new(empty).size(12.0).color(p.muted));
        ui.add_space(6.0);
    } else {
        egui::ScrollArea::vertical()
            .id_salt("fs_nearby_media_list")
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for row in rows {
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

/// The Show list (`show_combo`); the choice made, if any.
fn filter_combo(ui: &mut egui::Ui, p: &Palette, shown: Filter) -> Option<Filter> {
    let mut chosen = None;
    let combo = egui::ComboBox::from_id_salt("fs_nearby_media_filter")
        .width(FILTER_WIDTH)
        .selected_text(RichText::new(shown.label()).size(12.0).color(p.ink))
        .truncate()
        .show_ui(ui, |ui| {
            for filter in Filter::ALL {
                let item = ui.selectable_label(filter == shown, filter.label());
                #[cfg(test)]
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::unique(("fs_nearby_media_filter_item", filter)), item.rect));
                if item.clicked() && filter != shown {
                    chosen = Some(filter);
                }
            }
        });
    let id = combo.response.id;
    ui.ctx().data_mut(|d| d.insert_temp(combo_id_key(), id));
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique("fs_nearby_media_filter_combo_rect"), combo.response.rect));
    chosen
}

/// Where the Show list and one of its entries were last drawn (frame
/// tests).
#[cfg(test)]
pub(super) fn filter_rects(ctx: &egui::Context, filter: Filter) -> (Option<egui::Rect>, Option<egui::Rect>) {
    ctx.data(|d| {
        (
            d.get_temp(egui::Id::unique("fs_nearby_media_filter_combo_rect")),
            d.get_temp(egui::Id::unique(("fs_nearby_media_filter_item", filter))),
        )
    })
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
        let name_width = (ui.available_width() - STATE_WIDTH - 3.0 * (BUTTON + spacing) - (SLIDER + spacing) - spacing).max(40.0);
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
            ui.add(widgets::elided_label(ui, name, name_width).sense(egui::Sense::click()))
        });
        // a double click on the name zooms on it, as on a row of the list there
        if name.inner.double_clicked() && row.place.zoomable && !row.place.zoomed {
            out.push(Action::Zoom(row.source));
        }
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
        // the zoom: none for the parcel's media and music (include_zoom),
        // greyed for an object the camera cannot frame (a HUD, not loaded)
        if matches!(row.source, Source::Prim { .. }) {
            let (icon, tip) = if row.place.zoomed {
                ("zoom-out-filled", tr!("fs-nearby-media-unzoom"))
            } else {
                ("zoom-in-filled", tr!("fs-nearby-media-zoom"))
            };
            if button(ui, row.source, "zoom", |ui| {
                widgets::icon_button(ui, p, icon, tip, row.place.zoomable)
            }) {
                out.push(if row.place.zoomed {
                    Action::Unzoom
                } else {
                    Action::Zoom(row.source)
                });
            }
        } else {
            ui.add_space(BUTTON);
        }
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
            // its own volume, as the media controls' slider (VolumeScale::Gain)
            let mut volume = row.volume;
            let slider = button_response(ui, row.source, "volume", |ui| {
                ui.spacing_mut().slider_width = SLIDER;
                widgets::volume_slider(ui, &mut volume, widgets::VolumeScale::Gain, enabled, |s| {
                    s.custom_parser(aurora_i18n::parse_number).trailing_fill(true).show_value(false)
                })
                .on_hover_text(tr!("fs-nearby-media-volume"))
            });
            if slider.changed() && enabled {
                out.push(Action::SetVolume(row.source, volume));
            }
        } else {
            // the music's mute and volume are the volume panel's
            ui.add_space(BUTTON + spacing + SLIDER);
        }
    });
}

/// [`button`] for a widget whose whole response is wanted (a slider).
fn button_response(
    ui: &mut egui::Ui,
    source: Source,
    kind: &'static str,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let r = ui.scope(add);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique(("fs_nearby_media_button", source, kind)), r.inner.rect));
    #[cfg(not(test))]
    let _ = (source, kind);
    r.inner
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
