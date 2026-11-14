//! "New IM" and "Friends" in the host's chats (session-host plan,
//! checkpoint 11): the shared chat's character tab and each monitor's
//! chat part have the two buttons beside their chips, and a sheet in the
//! lines' place (a list, as "+N"'s: it scrolls in a window too small for
//! a menu).
//!
//! - **Friends**: the character's friends, online first, as its viewer
//!   tells them; a row's buttons ask that viewer to open an instant
//!   message, or the friend's profile (the host then brings the
//!   character forward: the profile is in its own window).
//! - **New IM**: the character's groups, then its friends, searched by
//!   name; a click on a row asks the viewer to open that conversation.
//!
//! The rules (whose list this is, what is asked, the answer) are the
//! `folderstorm-session` crate's `contacts::Book`; nothing here decides,
//! and nothing of the lists is written anywhere.

use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::contacts::{Book, Found, Notice, Which, Why};
use ::folderstorm_session::ipc::{Open, OpenRefusal};
use aurora_i18n::tr;
use egui::{RichText, Sense, Vec2};

/// A row's height, and its buttons' size.
const ROW: f32 = 24.0;
const ROW_BUTTON: Vec2 = Vec2::new(26.0, 22.0);

/// The sheet of one window for one character.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Sheet {
    pub which: Which,
    pub query: String,
}

/// What a click in the sheet asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Asked {
    /// Ask the viewer to open this, named so.
    pub open: Option<(Open, String)>,
    /// The sheet closes.
    pub close: bool,
}

/// The two buttons, beside the chips: each opens its sheet, or closes it
/// when it is the one open. True when the sheet changed.
pub(super) fn buttons(ui: &mut egui::Ui, p: &Palette, sheet: &mut Option<Sheet>, name: &str) -> bool {
    let mut changed = false;
    for (which, icon, tip) in [
        (
            Which::Everyone,
            "message-circle-plus",
            tr!("folderstorm-contacts-new-im-help", name = name.to_owned()),
        ),
        (
            Which::Friends,
            "users",
            tr!("folderstorm-contacts-friends-help", name = name.to_owned()),
        ),
    ] {
        let on = sheet.as_ref().is_some_and(|s| s.which == which);
        let kind = if on { ActionKind::Primary } else { ActionKind::Flat };
        if widgets::action_button(ui, p, icon, "", kind, ROW_BUTTON)
            .on_hover_text(tip)
            .clicked()
        {
            *sheet = (!on).then(|| Sheet {
                which,
                query: String::new(),
            });
            changed = true;
        }
    }
    changed
}

/// The width the two buttons take.
pub(super) fn buttons_width(ui: &egui::Ui) -> f32 {
    2.0 * ROW_BUTTON.x + 2.0 * ui.spacing().item_spacing.x
}

fn refusal(reason: OpenRefusal) -> &'static str {
    match reason {
        OpenRefusal::NotInWorld => tr!("folderstorm-chat-refused-not-in-world"),
        OpenRefusal::Stale => tr!("folderstorm-chat-refused-stale"),
        OpenRefusal::Restricted => tr!("folderstorm-contacts-refused-restricted"),
        OpenRefusal::NotAGroup => tr!("folderstorm-contacts-refused-not-a-group"),
        OpenRefusal::Unknown => tr!("folderstorm-contacts-refused-unknown"),
        OpenRefusal::Repeated => tr!("folderstorm-chat-refused-repeated"),
    }
}

/// What the host says of a request that did not open.
pub(super) fn notice_text(notice: &Notice, character: &str) -> String {
    let why = match notice.why {
        Why::Refused(reason) => refusal(reason),
        Why::NoAnswer => tr!("folderstorm-contacts-no-answer"),
        Why::NotSent => tr!("folderstorm-contacts-not-sent"),
    };
    tr!(
        "folderstorm-contacts-not-opened",
        name = character.to_owned(),
        who = notice.name.clone(),
        why = why
    )
}

/// The notice of the window's last request, with a cross; true when it
/// was dismissed. Nothing when there is none.
pub(super) fn notice_row(ui: &mut egui::Ui, p: &Palette, book: &Book, window: u8, name: &dyn Fn(usize) -> String) -> bool {
    let Some(notice) = book.notice(window) else {
        return false;
    };
    let text = notice_text(notice, &name(notice.slot));
    let mut dismissed = false;
    ui.horizontal(|ui| {
        dismissed = widgets::icon_button(ui, p, "x", tr!("folderstorm-contacts-dismiss"), true);
        ui.add(egui::Label::new(RichText::new(&text).color(p.amber)).truncate())
            .on_hover_text(&text);
    });
    dismissed
}

/// One row: the state's dot, the name, its buttons.
fn row(ui: &mut egui::Ui, p: &Palette, book: &Book, slot: usize, found: &Found<'_>, which: Which, asked: &mut Asked) {
    let contact = found.contact;
    let open = found.open();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, ui.visuals().window_corner_radius, p.raised);
    }
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(4.0, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut inner;
    if found.group {
        super::faces::sign(ui, "users", p.muted, tr!("folderstorm-contacts-group"));
    } else {
        widgets::status_dot(ui, if contact.online { p.success } else { p.muted_dim });
    }
    let opening = book.opening(slot, open);
    let buttons = if found.group || which == Which::Everyone { 1.0 } else { 2.0 };
    let room = (ui.available_width() - buttons * (ROW_BUTTON.x + 4.0)).max(20.0);
    let color = if contact.online || found.group { p.ink } else { p.muted };
    // (the name from the left, cut to its room, whole in the hover)
    ui.allocate_ui_with_layout(Vec2::new(room, ROW - 4.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(room);
        ui.add(egui::Label::new(RichText::new(&contact.name).color(color)).truncate())
            .on_hover_text(&contact.name);
    });
    if opening {
        ui.label(RichText::new("…").color(p.muted))
            .on_hover_text(tr!("folderstorm-contacts-opening"));
        return;
    }
    let im_tip = if found.group {
        tr!("folderstorm-contacts-group-chat")
    } else {
        tr!("folderstorm-contacts-im")
    };
    if widgets::action_button(ui, p, "message-circle-plus", "", ActionKind::Flat, ROW_BUTTON)
        .on_hover_text(im_tip)
        .clicked()
    {
        asked.open = Some((open, contact.name.clone()));
    }
    if !found.group
        && which == Which::Friends
        && widgets::action_button(ui, p, "user", "", ActionKind::Flat, ROW_BUTTON)
            .on_hover_text(tr!("folderstorm-contacts-profile"))
            .clicked()
    {
        asked.open = Some((Open::Profile(contact.who), contact.name.clone()));
    }
    // (New IM: a click anywhere on the row opens it)
    if which == Which::Everyone && response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        asked.open = Some((open, contact.name.clone()));
    }
}

fn section(ui: &mut egui::Ui, p: &Palette, text: String) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).size(11.5).strong().color(p.muted));
}

/// The sheet, in the lines' place: a search, then the rows. `name`: the
/// character as the host shows it; `in_world`: it has a login.
pub(super) fn sheet(ui: &mut egui::Ui, p: &Palette, book: &Book, slot: usize, name: &str, in_world: bool, sheet: &mut Sheet) -> Asked {
    let mut asked = Asked::default();
    ui.horizontal(|ui| {
        let heading = match sheet.which {
            Which::Friends => tr!("folderstorm-contacts-friends-of", name = name.to_owned()),
            Which::Everyone => tr!("folderstorm-contacts-new-im-as", name = name.to_owned()),
        };
        ui.add(egui::Label::new(RichText::new(heading).strong().color(p.violet_light)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            asked.close = widgets::icon_button(ui, p, "x", tr!("common-close"), true);
        });
    });
    if !in_world {
        ui.label(RichText::new(tr!("folderstorm-chat-blocked-not-in-world", name = name.to_owned())).color(p.muted));
        return asked;
    }
    if !book.known(slot) {
        ui.label(RichText::new(tr!("folderstorm-contacts-waiting", name = name.to_owned())).color(p.muted));
        return asked;
    }
    let hint = match sheet.which {
        Which::Friends => tr!("folderstorm-contacts-search-friends"),
        Which::Everyone => tr!("folderstorm-contacts-search-everyone"),
    };
    // (in a row: the field keeps its own height)
    ui.horizontal(|ui| {
        let width = ui.available_width();
        widgets::clearable_search_box(ui, p, &mut sheet.query, hint, width);
    });
    ui.add_space(2.0);
    let rows = book.search(slot, &sheet.query, sheet.which);
    if rows.is_empty() {
        let empty = if sheet.query.trim().is_empty() {
            tr!("folderstorm-contacts-none", name = name.to_owned())
        } else {
            tr!("folderstorm-contacts-no-match").to_owned()
        };
        ui.label(RichText::new(empty).color(p.muted));
    }
    let online = rows.iter().filter(|f| !f.group && f.contact.online).count();
    let friends = rows.iter().filter(|f| !f.group).count();
    let groups = rows.len() - friends;
    let mut heading: Option<u8> = None;
    for found in &rows {
        // a heading where the kind of row changes
        let kind = match (found.group, found.contact.online, sheet.which) {
            (true, _, _) => 0,
            (false, _, Which::Everyone) => 1,
            (false, true, Which::Friends) => 2,
            (false, false, Which::Friends) => 3,
        };
        if heading != Some(kind) {
            heading = Some(kind);
            let text = match kind {
                0 => tr!("folderstorm-contacts-groups", count = groups as i64),
                1 => tr!("folderstorm-contacts-friends", count = friends as i64),
                2 => tr!("folderstorm-contacts-online", count = online as i64),
                _ => tr!("folderstorm-contacts-offline", count = (friends - online) as i64),
            };
            section(ui, p, text);
        }
        row(ui, p, book, slot, found, sheet.which, &mut asked);
    }
    let (more_friends, more_groups) = book.more(slot);
    let more = more_friends + if sheet.which == Which::Everyone { more_groups } else { 0 };
    if more > 0 {
        ui.label(
            RichText::new(tr!("folderstorm-contacts-more", count = i64::from(more), name = name.to_owned()))
                .italics()
                .color(p.muted),
        );
    }
    asked
}

/// The chips' row with the two buttons at its end: the chips take what the
/// buttons leave. Returns the chips as drawn, and whether the sheet
/// changed.
pub(super) fn chips_and_buttons(
    ui: &mut egui::Ui,
    p: &Palette,
    chips: impl FnOnce(&mut egui::Ui) -> super::chips::Shown,
    sheet: &mut Option<Sheet>,
    name: &str,
) -> (super::chips::Shown, bool) {
    let mut changed = false;
    let width = ui.available_width();
    let room = buttons_width(ui);
    let shown = ui
        .horizontal_top(|ui| {
            let shown = ui
                .allocate_ui(Vec2::new((width - room).max(60.0), super::chips::HEIGHT), |ui| {
                    ui.set_width((width - room).max(60.0));
                    chips(ui)
                })
                .inner;
            changed = buttons(ui, p, sheet, name);
            shown
        })
        .inner;
    (shown, changed)
}

/// What the window asks when its sheet asked to open something: the
/// profile brings the character forward.
pub(super) fn wants_switch(open: Open) -> bool {
    matches!(open, Open::Profile(_))
}
