//! The favorites strip (FEATURES.md row 1.6; folderstorm/plans/workspaces.md
//! "1.6 in detail"): the favorite layouts and workspaces as pills on the
//! row under the top bar, one click to apply.
//!
//! Folderstorm (C++) first gave them a row of their own under the
//! navigation and landmark bars, then, at its user's request, put them on
//! the landmark row itself after a `|` divider, with what does not fit in
//! *More…* (doc/help.md "Workspaces"; doc/plans/workspace-enhancements.md,
//! 1 and the follow-up of October 4). Hidden until enabled, in mouselook
//! and with the interface.
//!
//! Here the landmark row is the favourites' row of `ui/bars/favorites.rs`,
//! the same under the islands and under the Legacy bars. Shown, it holds
//! our pills at its right end ([`in_row`], at most [`SHARE`] of it); folded
//! to its tab or not there, a short island of ours stands in its place
//! ([`own_row`]), and the windows stay under it as under the row
//! (`islands::favorites_row_bottom`). Both are called by the top bar's
//! code, so the strip goes away and peeks with the top bar.
//!
//! The bars are drawn without the settings in reach: the strip reads the
//! view the frame call publishes for the menu (`super::menu::View`) and
//! leaves the menu's requests, so a click here is a click in World ›
//! Layouts.

use super::menu::{self, Favorite, Request, View};
use crate::theme::Palette;
use crate::ui::bars::favorites::{ROW_HEIGHT, row_bounds};
use crate::ui::bars::islands::{self, GAP, ICON, MARGIN, PAD, SLOT};
use crate::ui::icons::Icons;
use aurora_i18n::tr;
use egui::{Color32, FontId, Id, Pos2, Rect, Sense, Vec2, pos2, vec2};
use std::sync::Arc;

/// The landmarks' pills are this high (`favorites.rs`, `PILL_H`).
const PILL_H: f32 = 26.0;
const PILL_ICON: f32 = 14.0;
const TEXT: f32 = 12.0;
/// A name longer than this is cut on its pill; the tooltip has all of it.
pub(super) const NAME_CHARS: usize = 18;
/// What the strip may take of the landmark row.
pub(super) const SHARE: f32 = 0.5;
/// The room of the divider between the landmarks and the strip.
pub(super) const DIVIDER: f32 = 9.0;

const ICON_MORE: &str = "dots";
const ICON_OVERFLOW: &str = "chevrons-right";
const ICON_HIDE: &str = "eye-off";

/// The name of the area of the strip's own island (`islands::area`).
pub(super) const ISLAND: &str = "folderstorm_strip";

fn hidden_id() -> Id {
    Id::unique("folderstorm_strip_hidden")
}

fn hosted_id() -> Id {
    Id::unique("folderstorm_strip_hosted")
}

/// The camera is in mouselook, or the interface is put away: the strip is
/// not drawn, even where the bars are (*Show the interface in mouselook*).
pub(super) fn set_hidden(ctx: &egui::Context, hidden: bool) {
    ctx.data_mut(|d| d.insert_temp(hidden_id(), hidden));
}

/// The view, when the strip is to be drawn.
fn shown(ctx: &egui::Context) -> Option<Arc<View>> {
    if ctx.data(|d| d.get_temp::<bool>(hidden_id())).unwrap_or(false) {
        return None;
    }
    Some(menu::view(ctx)).filter(|view| view.strip)
}

/// A name for a pill.
pub(super) fn short(name: &str) -> String {
    if name.chars().count() <= NAME_CHARS {
        return name.to_owned();
    }
    let mut cut: String = name.chars().take(NAME_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// How many of the pills, each `widths` wide, stand in `room` with *More…*
/// after them; `more(left)` is its width when `left` favorites are in it.
/// *More…* is always there, so its room is kept first.
pub(super) fn fits(widths: &[f32], more: impl Fn(usize) -> f32, room: f32) -> usize {
    let mut used = 0.0;
    let mut count = 0;
    while count < widths.len() {
        let next = used + widths[count] + GAP;
        if next + more(widths.len() - count - 1) > room {
            break;
        }
        used = next;
        count += 1;
    }
    count
}

/// The width of `count` pills and *More…* after them.
fn width(widths: &[f32], count: usize, more: f32) -> f32 {
    widths[..count].iter().map(|width| width + GAP).sum::<f32>() + more
}

/// The strip measured for a room: each pill's width, how many fit, and
/// the width of *More…*.
struct Measured {
    widths: Vec<f32>,
    count: usize,
    more: f32,
}

impl Measured {
    fn of(painter: &egui::Painter, view: &View, room: f32) -> Self {
        let text = |text: String| {
            painter
                .layout_no_wrap(text, FontId::proportional(TEXT), Color32::PLACEHOLDER)
                .size()
                .x
                .ceil()
        };
        let widths: Vec<f32> = view
            .favorites
            .iter()
            .map(|favorite| 6.0 + PILL_ICON + 5.0 + text(short(&favorite.name)) + 9.0)
            .collect();
        let more = |left: usize| match left {
            0 => SLOT,
            left => 6.0 + ICON + 6.0 + text(tr!("fs-strip-more", n = left as i64).to_string()) + 6.0,
        };
        let count = fits(&widths, more, room);
        let more = more(widths.len() - count);
        Measured { widths, count, more }
    }

    fn width(&self) -> f32 {
        width(&self.widths, self.count, self.more)
    }
}

fn paint_icon(ui: &egui::Ui, icons: &Icons, name: &str, center: Pos2, side: f32, tint: Color32) {
    if let Some(icon) = icons.get(name) {
        ui.painter().image(
            icon.id(),
            Rect::from_center_size(center, Vec2::splat(side)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }
}

fn kind_of(favorite: &Favorite) -> &'static str {
    match favorite.kind {
        super::pick::Kind::Layout => tr!("fs-strip-layout"),
        super::pick::Kind::Workspace => tr!("fs-strip-workspace"),
    }
}

/// The pills from `x` on, centred on `cy`, then *More…*.
fn pills(ui: &mut egui::Ui, p: &Palette, icons: &Icons, view: &View, measured: &Measured, mut x: f32, cy: f32) {
    let ctx = ui.ctx().clone();
    for (place, favorite) in view.favorites.iter().take(measured.count).enumerate() {
        let pill = Rect::from_min_size(pos2(x, cy - PILL_H / 2.0), vec2(measured.widths[place], PILL_H));
        x = pill.right() + GAP;
        let response = ui.interact(pill, Id::unique(("folderstorm_strip_pill", place)), Sense::click());
        islands::slot_face(ui, p, pill, response.hovered(), favorite.in_use);
        paint_icon(
            ui,
            icons,
            favorite.icon(),
            pos2(pill.left() + 6.0 + PILL_ICON / 2.0, cy),
            PILL_ICON,
            if favorite.in_use { p.ink } else { p.muted },
        );
        let name = ui.painter().layout_no_wrap(short(&favorite.name), FontId::proportional(TEXT), p.ink);
        crate::ui::widgets::paint_ink_left(ui.painter(), name, pos2(pill.left() + 6.0 + PILL_ICON + 5.0, cy), p.ink);
        let response = response.on_hover_ui(|ui| {
            ui.label(egui::RichText::new(&favorite.name).size(TEXT).strong().color(p.ink));
            let state = if favorite.in_use {
                format!("{} · {}", kind_of(favorite), tr!("fs-layouts-active"))
            } else {
                kind_of(favorite).to_owned()
            };
            ui.label(egui::RichText::new(state).size(TEXT).color(p.ink));
            ui.label(egui::RichText::new(tr!("fs-strip-pill-hint")).size(11.5).color(p.muted));
        });
        if response.clicked() {
            menu::ask(&ctx, favorite.apply());
        }
    }
    let left = view.favorites.len() - measured.count;
    let more = Rect::from_min_size(pos2(x, cy - PILL_H / 2.0), vec2(measured.more, PILL_H));
    let response = ui.interact(more, Id::unique("folderstorm_strip_more"), Sense::click());
    let open = egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
    let hot = response.hovered() || open;
    islands::slot_face(ui, p, more, hot, false);
    let ink = if hot { p.ink } else { p.muted };
    if left == 0 {
        paint_icon(ui, icons, ICON_MORE, more.center(), ICON, ink);
    } else {
        let icon = pos2(more.left() + 6.0 + ICON / 2.0, cy);
        paint_icon(ui, icons, ICON_OVERFLOW, icon, ICON, ink);
        let text = ui.painter().layout_no_wrap(
            tr!("fs-strip-more", n = left as i64).to_string(),
            FontId::proportional(TEXT),
            ink,
        );
        crate::ui::widgets::paint_ink_left(ui.painter(), text, pos2(icon.x + ICON / 2.0 + 6.0, cy), ink);
    }
    crate::ui::menu::dropdown_placed(
        &response,
        p,
        egui::PopupCloseBehavior::CloseOnClick,
        Some((egui::RectAlign::BOTTOM_START, 10.0)),
        |ui| more_body(ui, p, view, measured.count),
    );
    if !open {
        response.on_hover_text(tr!("fs-strip-more-tip"));
    }
}

/// *More…*: the favorites after the first `count`, then the switcher, the
/// way back and the strip put away.
pub(super) fn more_body(ui: &mut egui::Ui, p: &Palette, view: &View, count: usize) {
    use crate::ui::menu as m;
    let mut request = None;
    for favorite in view.favorites.iter().skip(count) {
        if m::check(ui, p, favorite.icon(), &favorite.name, favorite.in_use, true) {
            request = Some(favorite.apply());
        }
    }
    if view.favorites.len() > count {
        m::separator(ui, p);
    }
    if m::item(ui, p, menu::ICON_OPEN, tr!("fs-layouts-menu-open")) {
        request = Some(Request::Switcher);
    }
    if m::item_if(ui, p, menu::ICON_PREVIOUS, tr!("fs-layouts-previous"), view.previous) {
        request = Some(Request::Previous);
    }
    if m::item(ui, p, ICON_HIDE, tr!("fs-strip-hide")) {
        request = Some(Request::Strip(false));
    }
    if let Some(request) = request {
        menu::ask(ui.ctx(), request);
    }
}

/// On the landmark row (the hook of `ui/bars/favorites.rs`, `row`): the
/// strip at the right end of `row`, before the button that folds it
/// (`fold`), after a divider. Gives back `fold`, or in its place where the
/// landmarks must stop: the row lays its pills out up to what it is given.
pub fn in_row(ui: &mut egui::Ui, p: &Palette, icons: &Icons, row: Rect, fold: Rect) -> Rect {
    let Some(view) = shown(ui.ctx()) else {
        return fold;
    };
    let right = fold.left() - GAP;
    let room = ((right - row.left() - PAD) * SHARE - DIVIDER).max(0.0);
    let measured = Measured::of(ui.painter(), &view, room);
    let left = right - measured.width();
    let cy = row.center().y;
    let x = left - DIVIDER / 2.0;
    ui.painter().vline(
        x.round() + 0.5,
        cy - PILL_H / 2.0 + 4.0..=cy + PILL_H / 2.0 - 4.0,
        crate::ui::widgets::divider_stroke(p),
    );
    pills(ui, p, icons, &view, &measured, left, cy);
    let frame = ui.ctx().cumulative_frame_nr();
    ui.ctx().data_mut(|d| d.insert_temp(hosted_id(), frame));
    // (the row stops its own pills `GAP` before what it takes for the button)
    Rect::from_min_max(pos2(left - DIVIDER + GAP, fold.top()), fold.max)
}

/// After the favourites' row (the hook of `ui/bars/islands.rs` and of the
/// Legacy top bar in `ui/bars.rs`), `under` being what the row stands
/// under: when the row did not hold the strip in this frame (it is folded
/// to its tab, or the favourites are in the menu or the dock), an island
/// of our own where the row would be, beside the tab when there is one.
/// The windows stay under it.
pub fn own_row(ctx: &egui::Context, p: &Palette, icons: &Icons, under: Rect) {
    let Some(view) = shown(ctx) else {
        return;
    };
    if ctx.data(|d| d.get_temp::<u64>(hosted_id())) == Some(ctx.cumulative_frame_nr()) {
        return;
    }
    let full = row_bounds(under, false);
    // (the row's bottom is known while its tab is drawn)
    let left = match islands::favorites_row_bottom(ctx) {
        Some(_) => row_bounds(under, true).right() + MARGIN,
        None => full.left(),
    };
    let room = (full.right() - left - PAD * 2.0).max(0.0);
    let measured = Measured::of(&ctx.layer_painter(egui::LayerId::background()), &view, room);
    let rect = Rect::from_min_size(pos2(left, full.top()), vec2(measured.width() + PAD * 2.0, ROW_HEIGHT));
    islands::area(ctx, p, ISLAND, rect, |ui| {
        // (the island takes the pointer between its pills, as the row does)
        ui.interact(rect, Id::unique("folderstorm_strip_back"), Sense::click());
        pills(ui, p, icons, &view, &measured, rect.left() + PAD, rect.center().y);
    });
    islands::set_favorites_row_bottom(ctx, Some(rect.bottom()));
    #[cfg(test)]
    ctx.data_mut(|d| d.insert_temp(Id::unique("folderstorm_strip_own"), (ctx.cumulative_frame_nr(), rect)));
}

/// The strip's own island, when the last frame drew it.
#[cfg(test)]
pub(super) fn own_island(ctx: &egui::Context) -> Option<Rect> {
    let (frame, rect) = ctx.data(|d| d.get_temp::<(u64, Rect)>(Id::unique("folderstorm_strip_own")))?;
    (frame + 1 == ctx.cumulative_frame_nr()).then_some(rect)
}
