//! A character's conversations as chips (session-host plan, checkpoint
//! 9), in the shared chat and in each monitor's chat: one drawing for
//! both. The rules (the order, the names that shrink to fit, two rows at
//! most and "+N" for the rest) are the `folderstorm-session` crate's
//! (`chips`); this file measures the texts, draws the chips where the
//! rules put them and returns the clicks.
//!
//! A chip looks like the viewer's filter pills (`ui/widgets/pills.rs`),
//! with its conversation's unread count and, on every chip but the
//! nearby chat's, a cross that closes the conversation **in the host
//! only** (`chat::Chat::close`): nothing is said to the character's
//! viewer.
//!
//! "+N" opens a list of the conversations that have no chip, drawn by the
//! caller in the lines' place ([`list`]): a list scrolls in a window too
//! small for a menu.

use super::chat_ui;
use crate::theme::Palette;
use crate::ui::{menu, widgets};
use ::folderstorm_session::chat::{Chat, Thread};
use ::folderstorm_session::chips::{self, Item, Widths};
use ::folderstorm_session::ipc::{Conversation, SessionKind};
use aurora_i18n::tr;
use egui::{FontId, Pos2, Rect, Sense, Vec2};

/// A chip's height, the room between two chips and between two rows.
pub(super) const HEIGHT: f32 = 26.0;
const GAP: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
/// Inside a chip: the room at its ends, its icon and the room after it,
/// the room before its count, its cross and the room before it.
const PAD: f32 = 9.0;
const ICON: f32 = 13.0;
const ICON_GAP: f32 = 5.0;
const COUNT_GAP: f32 = 5.0;
const CROSS: f32 = 12.0;
const CROSS_GAP: f32 = 5.0;

/// What the clicks on the chips ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Asked {
    /// Show that conversation.
    pub select: Option<Conversation>,
    /// Close it in the host.
    pub close: Option<Conversation>,
    /// Pin it, or let it go.
    pub pin: Option<(Conversation, bool)>,
    /// "+N" was clicked: its list opens or closes.
    pub more: bool,
}

impl Asked {
    /// Carries the clicks out for `slot`: `shown` is the conversation its
    /// window shows, `more` whether the "+N" list is open. True when
    /// something changed.
    pub fn apply(&self, chat: &mut Chat, slot: usize, shown: &mut Conversation, more: &mut bool) -> bool {
        let mut acted = false;
        if let Some(conversation) = self.select {
            *shown = conversation;
            *more = false;
            acted = true;
        }
        if let Some((conversation, pinned)) = self.pin {
            acted |= chat.set_pinned(slot, conversation, pinned);
        }
        if let Some(conversation) = self.close
            && chat.close(slot, conversation)
        {
            if *shown == conversation {
                // the nearby chat, or whatever is first
                *shown = chat.open_threads(slot).next().map(|t| t.conversation).unwrap_or_default();
            }
            acted = true;
        }
        if self.more {
            *more = !*more;
            acted = true;
        }
        acted
    }
}

/// The chips as they were drawn.
#[derive(Clone, Debug, Default)]
pub(super) struct Shown {
    pub asked: Asked,
    /// The conversations behind "+N", in their order.
    pub hidden: Vec<Conversation>,
}

fn icon(thread: &Thread) -> &'static str {
    match (thread.pinned(), thread.kind) {
        (true, _) => "pinned-filled",
        (false, SessionKind::Nearby) => "message-circle-filled",
        (false, SessionKind::Resident) => "user",
        (false, SessionKind::Group | SessionKind::Conference) => "users",
    }
}

fn font(ui: &egui::Ui) -> FontId {
    FontId::new(12.0, crate::ui::fonts::semibold(ui.ctx()))
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font(ui), egui::Color32::PLACEHOLDER)
        .size()
        .x
}

/// "+N", and how many of them have something unread.
fn more_label(hidden: usize, unread: usize) -> String {
    match unread {
        0 => format!("+{hidden}"),
        unread => format!("+{hidden} · {}", chat_ui::count_text(unread as u32)),
    }
}

/// The background and the outline of a chip.
fn face(ui: &egui::Ui, p: &Palette, rect: Rect, on: bool, hovered: bool) {
    let radius = ui.visuals().window_corner_radius;
    let fill = if on {
        p.violet.gamma_multiply(0.28)
    } else if hovered {
        p.raised
    } else {
        p.bar
    };
    ui.painter().rect_filled(rect, radius, fill);
    if on {
        ui.painter().rect_stroke(
            rect.shrink(0.5),
            radius,
            egui::Stroke::new(1.0, p.violet_light.gamma_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }
}

fn paint_icon(ui: &egui::Ui, name: &str, center: Pos2, side: f32, tint: egui::Color32) {
    if let Some(texture) = crate::ui::icons::global(name) {
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter()
            .image(texture.id(), Rect::from_center_size(center, Vec2::splat(side)), uv, tint);
    }
}

/// The chips of `slot`'s conversations, across the width `ui` has.
/// `selected`: the conversation on screen; `list_open`: the "+N" list is
/// shown (its chip is lit).
pub(super) fn row(ui: &mut egui::Ui, p: &Palette, chat: &Chat, slot: usize, selected: Option<Conversation>, list_open: bool) -> Shown {
    let threads: Vec<&Thread> = chat.open_threads(slot).collect();
    if threads.is_empty() {
        return Shown::default();
    }
    let titles: Vec<String> = threads.iter().map(|thread| chat_ui::title(thread)).collect();
    let items: Vec<Item> = threads
        .iter()
        .zip(&titles)
        .map(|(thread, title)| Item {
            nearby: thread.conversation == Conversation::Nearby,
            pinned: thread.pinned(),
            unread: thread.unread,
            recency: thread.recency(),
            selected: selected == Some(thread.conversation),
            chars: title.chars().count(),
        })
        .collect();
    let counts: Vec<Option<String>> = threads
        .iter()
        .map(|thread| (thread.unread > 0).then(|| chat_ui::count_text(thread.unread)))
        .collect();
    let room = ui.available_width();
    // (the nearby chat's chip keeps its label: it is no name, and the one
    // chip that is always there)
    let label = |index: usize, cut: usize| {
        if items[index].nearby {
            std::borrow::Cow::Borrowed(titles[index].as_str())
        } else {
            chips::truncate(&titles[index], cut)
        }
    };
    // (measured, then laid out: the widths are kept for the drawing)
    let (layout, chip_widths, more_width) = {
        let chip = |index: usize, cut: usize| {
            let count = counts[index].as_ref().map_or(0.0, |count| COUNT_GAP + text_width(ui, count));
            let cross = if items[index].nearby { 0.0 } else { CROSS_GAP + CROSS };
            PAD + ICON + ICON_GAP + text_width(ui, &label(index, cut)) + count + cross + PAD
        };
        let more = |hidden: usize, unread: usize| PAD + text_width(ui, &more_label(hidden, unread)) + PAD;
        let layout = chips::layout(
            &items,
            &Widths {
                room,
                gap: GAP,
                chip: &chip,
                more: &more,
            },
        );
        let chip_widths: Vec<f32> = (0..items.len()).map(|index| chip(index, layout.chars)).collect();
        let more_width = more(layout.hidden.len(), layout.hidden_unread);
        (layout, chip_widths, more_width)
    };
    let rows = layout.rows.len().max(1);
    let height = rows as f32 * HEIGHT + (rows - 1) as f32 * ROW_GAP;
    let (area, _) = ui.allocate_exact_size(Vec2::new(room, height), Sense::hover());
    let mut asked = Asked::default();
    let id = ui.scope_id().with(("folderstorm-chips", slot));
    let font = font(ui);
    let mut x = area.left();
    for (row_index, row) in layout.rows.iter().enumerate() {
        x = area.left();
        let top = area.top() + row_index as f32 * (HEIGHT + ROW_GAP);
        for index in row {
            let (thread, item) = (threads[*index], &items[*index]);
            let width = chip_widths[*index];
            let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(width, HEIGHT));
            x += width + GAP;
            let chip_id = id.with(thread.conversation);
            let mut response = ui.interact(rect, chip_id, Sense::click());
            face(ui, p, rect, item.selected, response.hovered());
            let ink = if item.selected || item.unread > 0 { p.ink } else { p.muted };
            let mut at = rect.left() + PAD;
            paint_icon(ui, icon(thread), Pos2::new(at + ICON / 2.0, rect.center().y), ICON, ink);
            at += ICON + ICON_GAP;
            let galley = ui
                .painter()
                .layout_no_wrap(label(*index, layout.chars).into_owned(), font.clone(), ink);
            let label_width = galley.size().x;
            widgets::paint_row_text(ui.painter(), galley, at, rect.center().y, ink);
            at += label_width;
            if let Some(count) = &counts[*index] {
                let galley = ui.painter().layout_no_wrap(count.clone(), font.clone(), p.violet_light);
                widgets::paint_row_text(ui.painter(), galley, at + COUNT_GAP, rect.center().y, p.violet_light);
            }
            if !item.nearby {
                let cross = Rect::from_center_size(Pos2::new(rect.right() - PAD - CROSS / 2.0, rect.center().y), Vec2::splat(16.0));
                let close = ui.interact(cross, chip_id.with("close"), Sense::click());
                paint_icon(ui, "x", cross.center(), CROSS, if close.hovered() { p.ink } else { p.muted });
                if close
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tr!("folderstorm-chips-close-help"))
                    .clicked()
                {
                    asked.close = Some(thread.conversation);
                }
                menu::context_menu(&response, p, |ui| {
                    let (pin_icon, pin) = if thread.pinned() {
                        ("pin-filled", tr!("folderstorm-chips-unpin"))
                    } else {
                        ("pinned-filled", tr!("folderstorm-chips-pin"))
                    };
                    if menu::item(ui, p, pin_icon, pin) {
                        asked.pin = Some((thread.conversation, !thread.pinned()));
                    }
                    if menu::item(ui, p, "x", tr!("folderstorm-chips-close")) {
                        asked.close = Some(thread.conversation);
                    }
                });
            }
            if !item.nearby && chips::is_cut(item.chars, layout.chars) {
                // the whole name, where it was cut
                response = response.on_hover_text(&titles[*index]);
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && asked.close.is_none() {
                asked.select = Some(thread.conversation);
            }
        }
    }
    if !layout.hidden.is_empty() {
        // "+N", at the end of the last row
        let label = more_label(layout.hidden.len(), layout.hidden_unread);
        let width = more_width;
        let top = area.top() + (rows - 1) as f32 * (HEIGHT + ROW_GAP);
        let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(width, HEIGHT));
        let response = ui.interact(rect, id.with("more"), Sense::click());
        face(ui, p, rect, list_open, response.hovered());
        let ink = if layout.hidden_unread > 0 { p.violet_light } else { p.ink };
        let galley = ui.painter().layout_no_wrap(label, font, ink);
        widgets::paint_row_text(ui.painter(), galley, rect.left() + PAD, rect.center().y, ink);
        let tip = match layout.hidden_unread {
            0 => tr!("folderstorm-chips-more-help", count = layout.hidden.len()),
            unread => tr!("folderstorm-chips-more-unread-help", count = layout.hidden.len(), unread = unread),
        };
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip)
            .clicked()
        {
            asked.more = true;
        }
    }
    Shown {
        asked,
        hidden: layout.hidden.iter().map(|index| threads[*index].conversation).collect(),
    }
}

/// The list "+N" opens, in the lines' place: the conversations that have
/// no chip, each with its whole name, its unread count and its cross. A
/// click on one shows it (it then has a chip: the one on screen always
/// has).
pub(super) fn list(ui: &mut egui::Ui, p: &Palette, chat: &Chat, slot: usize, hidden: &[Conversation]) -> Asked {
    let mut asked = Asked::default();
    ui.label(egui::RichText::new(tr!("folderstorm-chips-more-heading")).color(p.muted));
    ui.add_space(2.0);
    let font = font(ui);
    for conversation in hidden {
        let Some(thread) = chat.open_thread(slot, *conversation) else {
            continue;
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, ui.visuals().window_corner_radius, p.raised);
        }
        let ink = if thread.unread > 0 { p.ink } else { p.muted };
        let mut at = rect.left() + PAD;
        paint_icon(ui, icon(thread), Pos2::new(at + ICON / 2.0, rect.center().y), ICON, ink);
        at += ICON + ICON_GAP;
        let cross = Rect::from_center_size(Pos2::new(rect.right() - PAD - CROSS / 2.0, rect.center().y), Vec2::splat(16.0));
        let count = (thread.unread > 0).then(|| chat_ui::count_text(thread.unread));
        let count_width = count.as_ref().map_or(0.0, |count| COUNT_GAP + text_width(ui, count));
        let name_room = (cross.left() - CROSS_GAP - count_width - at).max(0.0);
        let title = chat_ui::title(thread);
        let cut = widgets::paint_elided(ui, &title, Pos2::new(at, rect.center().y), name_room, font.clone(), ink);
        if let Some(count) = count {
            let galley = ui.painter().layout_no_wrap(count, font.clone(), p.violet_light);
            let x = cross.left() - CROSS_GAP - galley.size().x;
            widgets::paint_row_text(ui.painter(), galley, x, rect.center().y, p.violet_light);
        }
        let close = ui.interact(cross, response.id.with("close"), Sense::click());
        paint_icon(ui, "x", cross.center(), CROSS, if close.hovered() { p.ink } else { p.muted });
        if close
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr!("folderstorm-chips-close-help"))
            .clicked()
        {
            asked.close = Some(*conversation);
        }
        let response = if cut { response.on_hover_text(&title) } else { response };
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && asked.close.is_none() {
            asked.select = Some(*conversation);
        }
    }
    asked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_n_says_how_many_have_something_unread() {
        assert_eq!(more_label(3, 0), "+3");
        assert_eq!(more_label(12, 2), "+12 · 2");
        assert_eq!(more_label(120, 120), "+120 · 99+");
    }

    /// Every icon this file names is one the viewer embeds.
    #[test]
    fn the_chips_icons_exist() {
        for name in ["pinned-filled", "pin-filled", "message-circle-filled", "user", "users", "x"] {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
