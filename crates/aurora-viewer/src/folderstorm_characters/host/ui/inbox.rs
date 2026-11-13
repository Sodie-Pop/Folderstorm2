//! The window's side of checkpoint 7's attention inbox (session-host
//! plan, section 13): one list, across all characters, of what waits for
//! an answer in their own windows, each with its character, its age and
//! "Switch to answer". What is in it and in which order is the
//! `folderstorm-session` crate's (`inbox`); this file shows it.
//!
//! **Nothing is answered here.** A row brings that character forward, or
//! is dismissed: there is no accept and no decline, and nothing is said to
//! a viewer about a question. **Dismiss** (a row), **Dismiss all** and
//! **Dismiss all for** a character (the heading's menu) only take
//! questions out of the host's inbox, its counts with them (follow-ups to
//! checkpoint 7; the rules are the crate's `Inbox::dismiss`): they still
//! wait, unanswered, in the character's own window.
//!
//! The inbox is a panel like the chat or the monitors (checkpoint 6):
//! docked, it is a band at the top of the host's window, there only while
//! something waits, over whichever page the window shows; "In a window of
//! its own" gives it a window, whose title says how many wait. The count
//! is also beside its name in the host's window, wherever the inbox is.

use super::super::chat_ui::attention_kind;
use super::super::faces::{self, Face};
use super::{Act, Window_};
use crate::theme::Palette;
use crate::ui::menu;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::inbox::{Age, Waiting};
use ::folderstorm_session::ipc::{AttentionItem, MAX_SLOTS};
use ::folderstorm_session::panels::Panel;
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::Instant;

/// The docked band's rows scroll within this height.
const BAND_ROWS: f32 = 112.0;

/// The height of a row's buttons and of the heading's menu.
const ROW_BUTTON: Vec2 = Vec2::new(0.0, 22.0);

/// Below this width a row's Dismiss is its icon alone.
const NARROW_ROW: f32 = 560.0;

/// What a click in a row asks for.
enum RowAct {
    Switch,
    Dismiss,
}

/// A count as the host's other counters write it.
pub(super) fn count_text(n: u32) -> String {
    super::super::chat_ui::count_text(n)
}

fn age_text(age: Age) -> String {
    match age {
        Age::Now => tr!("folderstorm-inbox-age-now").to_owned(),
        Age::Minutes(minutes) => tr!("folderstorm-inbox-age-minutes", count = minutes),
        Age::Hours(hours) => tr!("folderstorm-inbox-age-hours", count = hours),
    }
}

/// A question in a line, as the chat's own lists word it.
fn question(item: &AttentionItem) -> String {
    if item.text.is_empty() {
        format!("{} — {}", attention_kind(item.kind), item.title)
    } else {
        format!("{} — {}: {}", attention_kind(item.kind), item.title, item.text)
    }
}

impl Window_ {
    /// The characters as they are shown, by slot.
    fn faces(&self) -> [Option<Face>; MAX_SLOTS] {
        let p = self.palette;
        std::array::from_fn(|slot| self.host.desk.slot(slot).map(|state| faces::face(&p, state)))
    }

    fn inbox_heading(&self, ui: &mut egui::Ui, p: &Palette, size: f32, acts: &mut Vec<Act>) {
        ui.horizontal_wrapped(|ui| {
            let count = self.host.inbox.count();
            let heading = match count {
                0 => tr!("folderstorm-inbox").to_owned(),
                n => format!("{} ({})", tr!("folderstorm-inbox"), count_text(n)),
            };
            if count > 0 {
                faces::sign(ui, "bell-filled", p.amber, tr!("folderstorm-inbox-help"));
            }
            ui.label(RichText::new(heading).size(size).strong().color(p.ink));
            ui.label(RichText::new(tr!("folderstorm-inbox-intro")).color(p.muted))
                .on_hover_text(tr!("folderstorm-inbox-help"));
            if count > 0 {
                self.dismiss_menu(ui, p, acts);
            }
        });
    }

    /// "Dismiss…": everything, or everything of one character.
    fn dismiss_menu(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<Act>) {
        let shown = self.faces();
        let inbox = &self.host.inbox;
        let label = tr!("folderstorm-inbox-dismiss-menu");
        let button =
            widgets::action_menu_button(ui, p, "bell-off", label, false, ROW_BUTTON.y).on_hover_text(tr!("folderstorm-inbox-dismiss-help"));
        menu::dropdown(&button, p, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            let all = tr!("folderstorm-inbox-dismiss-all", count = inbox.count());
            if menu::item(ui, p, "bell-off", &all) {
                acts.push(Act::InboxDismissAll);
            }
            menu::separator(ui, p);
            for (slot, face) in shown.iter().enumerate() {
                let (count, Some(face)) = (inbox.count_for(slot), face) else {
                    continue;
                };
                if count > 0 {
                    let text = tr!("folderstorm-inbox-dismiss-for", name = face.name.clone(), count = count);
                    if menu::item(ui, p, "bell-off", &text) {
                        acts.push(Act::InboxDismissSlot(slot));
                    }
                }
            }
        });
    }

    /// The inbox docked: a band at the top of the host's window.
    pub(super) fn inbox_band(&mut self, ui: &mut egui::Ui, p: &Palette, now: Instant, acts: &mut Vec<Act>) {
        self.inbox_heading(ui, p, 14.5, acts);
        self.inbox_list(ui, p, now, BAND_ROWS, acts);
    }

    /// The inbox in its own window.
    pub(super) fn inbox_window(&mut self, ui: &mut egui::Ui, p: &Palette, now: Instant, acts: &mut Vec<Act>) {
        self.inbox_heading(ui, p, 18.0, acts);
        ui.add_space(8.0);
        if self.host.inbox.is_empty() {
            ui.label(RichText::new(tr!("folderstorm-inbox-empty")).color(p.muted));
            return;
        }
        let room = ui.available_height();
        self.inbox_list(ui, p, now, room, acts);
    }

    /// What waits, oldest first (or the latest first): the character, the question, its age
    /// and the way to that character's window.
    fn inbox_list(&self, ui: &mut egui::Ui, p: &Palette, now: Instant, room: f32, acts: &mut Vec<Act>) {
        let shown = self.faces();
        let inbox = &self.host.inbox;
        let newest_first = self.host.desk.attention().newest_first;
        egui::ScrollArea::vertical()
            .id_salt("folderstorm-host-inbox-rows")
            .auto_shrink([false, true])
            .max_height(room)
            .show(ui, |ui| {
                // (checkpoint 10: or the latest first, as chosen)
                let mut waiting: Vec<_> = inbox.waiting().iter().collect();
                if newest_first {
                    waiting.reverse();
                }
                for waiting in waiting {
                    let Some(face) = shown.get(waiting.slot).and_then(Option::as_ref) else {
                        continue;
                    };
                    // (a row of its own per question: two rows' buttons are
                    // not one widget)
                    ui.push_id(("folderstorm-host-inbox-row", waiting.slot, waiting.item.id), |ui| match inbox_row(
                        ui, p, face, waiting, now,
                    ) {
                        Some(RowAct::Switch) => acts.push(Act::Switch(waiting.slot)),
                        Some(RowAct::Dismiss) => acts.push(Act::InboxDismiss(waiting.slot, waiting.item.id)),
                        None => {}
                    });
                }
                for (slot, face) in shown.iter().enumerate() {
                    let (more, Some(face)) = (inbox.more(slot), face) else {
                        continue;
                    };
                    if more > 0 {
                        let text = tr!("folderstorm-inbox-more", count = more, name = face.name.clone());
                        ui.label(RichText::new(text).color(p.muted));
                    }
                }
            });
    }

    /// The inbox's own window says in its title how many wait.
    pub(super) fn inbox_retitle(&mut self) {
        let count = self.host.inbox.count();
        if std::mem::replace(&mut self.inbox_titled, count) == count {
            return;
        }
        let title = self.pane_title(Panel::Inbox);
        if let Some(window) = self.panes[Panel::Inbox.index()].as_ref().and_then(|view| view.window.as_ref()) {
            window.set_title(&title);
        }
    }
}

/// One question's row, and what a click in it asks for.
fn inbox_row(ui: &mut egui::Ui, p: &Palette, face: &Face, waiting: &Waiting, now: Instant) -> Option<RowAct> {
    let mut asked = None;
    ui.horizontal(|ui| {
        faces::dot(ui, p, face.tint);
        let name = ui.label(RichText::new(&face.name).strong().color(face.tint.unwrap_or(p.ink)));
        if let Some(real) = &face.real {
            name.on_hover_text(real);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // (narrow, Dismiss is its icon: the question keeps some room)
            let dismissed = if ui.available_width() < NARROW_ROW {
                let tip = format!("{}: {}", tr!("folderstorm-inbox-dismiss"), tr!("folderstorm-inbox-dismiss-help"));
                widgets::icon_button(ui, p, "bell-off", &tip, true)
            } else {
                let dismiss = tr!("folderstorm-inbox-dismiss");
                widgets::action_button(ui, p, "bell-off", dismiss, ActionKind::Flat, ROW_BUTTON)
                    .on_hover_text(tr!("folderstorm-inbox-dismiss-help"))
                    .clicked()
            };
            if dismissed {
                asked = Some(RowAct::Dismiss);
            }
            let answer = tr!("folderstorm-chat-switch-to-answer");
            if widgets::action_button(ui, p, "arrows-exchange", answer, ActionKind::Flat, ROW_BUTTON)
                .on_hover_text(tr!("folderstorm-chat-switch-to-answer-help"))
                .clicked()
            {
                asked = Some(RowAct::Switch);
            }
            ui.label(RichText::new(age_text(Age::of(waiting.age(now)))).color(p.muted))
                .on_hover_text(tr!("folderstorm-inbox-age-help"));
            // the question takes what is left, cut short when it is long
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text = question(&waiting.item);
                ui.add(egui::Label::new(RichText::new(&text).color(p.ink)).truncate())
                    .on_hover_text(text);
            });
        });
    });
    asked
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::panels::Which;
    use super::super::tests::{draw, draw_in, window};
    use super::*;
    use ::folderstorm_session::identity::Colour;
    use ::folderstorm_session::ipc::{AttentionKind, AttentionList};
    use std::time::Duration;

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    fn painted(pixels: &[u32]) -> usize {
        let background = pixels[0];
        pixels.iter().filter(|p| **p != background).count()
    }

    fn asks(app: &mut Window_, slot: usize, id: u64, kind: AttentionKind, title: &str, text: &str, now: Instant) {
        let mut list: Vec<AttentionItem> = app.host.chat.attention(slot).0.to_vec();
        list.push(AttentionItem {
            id,
            kind,
            title: title.into(),
            text: text.into(),
        });
        app.host.chat.attention_list(
            slot,
            1,
            AttentionList {
                generation: 1,
                items: list,
                more: 0,
            },
        );
        app.host.sync_inbox(now);
    }

    #[test]
    fn the_inbox_lists_every_character_s_questions_oldest_first_with_their_age() {
        let start = Instant::now();
        let mut app = window(host());
        // nothing waits: no band, and the strip names the inbox without a count
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-inbox")));
        assert!(!shows(&drawn, tr!("folderstorm-inbox-intro")), "{drawn:?}");
        asks(&mut app, 1, 7, AttentionKind::Teleport, "Jane Doe", "Join me", start);
        asks(
            &mut app,
            0,
            3,
            AttentionKind::Script,
            "Door",
            "Open?",
            start + Duration::from_secs(120),
        );
        let now = start + Duration::from_secs(300);
        app.host.sync_inbox(now);
        assert_eq!(app.host.inbox.count(), 2);
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        let inbox = format!("{} (2)", tr!("folderstorm-inbox"));
        assert!(
            drawn.iter().filter(|t| t.contains(&inbox)).count() >= 2,
            "the badge and the heading: {drawn:?}"
        );
        let at = |text: &str| {
            drawn
                .iter()
                .position(|t| t.contains(text))
                .unwrap_or_else(|| panic!("{text} in {drawn:?}"))
        };
        assert!(at("Teleport — Jane Doe: Join me") < at("— Door: Open?"), "oldest first");
        assert!(shows(&drawn, tr!("folderstorm-chat-switch-to-answer")));
        assert!(painted(&pixels) > 10_000);
        // (the ages are worded from when the host first saw each)
        assert_eq!(app.host.inbox.waiting()[0].age(now), Duration::from_secs(300));
        assert_eq!(
            age_text(Age::of(Duration::from_secs(300))),
            tr!("folderstorm-inbox-age-minutes", count = 5u32)
        );
        assert_eq!(age_text(Age::Now), tr!("folderstorm-inbox-age-now"));
        assert!(!age_text(Age::Hours(2)).is_empty());
        // beside the inbox, the chat's "All" does not list them again
        assert!(app.chat.inbox_beside);
        assert!(!shows(&drawn, "has something to answer"), "{drawn:?}");
        // narrow, on the characters' page: the band is still there
        let (drawn, _) = draw(&mut app, 460, 760);
        assert!(shows(&drawn, "Teleport — Jane Doe: Join me"), "{drawn:?}");
    }

    #[test]
    fn nothing_is_answered_from_the_inbox_and_its_only_act_is_a_switch() {
        let now = Instant::now();
        let mut app = window(host());
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
        asks(&mut app, 1, 7, AttentionKind::Friendship, "Jane Doe", "Friends?", now);
        draw(&mut app, 1120, 760);
        // drawing asks for nothing
        assert!(app.host.desk.take_effects().is_empty());
        assert!(app.host.chat.take_effects().is_empty());
        // "Switch to answer": the desk's switch, and nothing for the chat
        app.apply(vec![Act::Switch(1)], now);
        assert_eq!(app.host.desk.active(), Some(1));
        assert!(
            app.host.chat.take_effects().is_empty(),
            "no word about the question goes to a viewer"
        );
        // the question still waits: only its viewer takes it off the list
        assert_eq!(app.host.inbox.count(), 1);
        app.host.chat.attention_list(
            1,
            1,
            AttentionList {
                generation: 1,
                items: Vec::new(),
                more: 0,
            },
        );
        app.host.sync_inbox(now);
        assert!(app.host.inbox.is_empty());
    }

    #[test]
    fn the_inbox_has_a_window_of_its_own_with_names_colours_and_what_is_not_listed() {
        let now = Instant::now();
        let mut app = window(host());
        app.host.desk.set_colour(1, Colour::Pink);
        assert_eq!(app.host.desk.set_alias(1, "Shop"), Ok(()));
        app.host.desk.set_panel_out(Panel::Inbox, true);
        app.panes[Panel::Inbox.index()] = Some(app.look.view());
        // empty: the window says so
        let (drawn, _) = draw_in(&mut app, Which::Pane(Panel::Inbox), 520, 380);
        assert!(
            shows(&drawn, tr!("folderstorm-inbox-empty")) && shows(&drawn, tr!("folderstorm-panel-dock")),
            "{drawn:?}"
        );
        assert_eq!(
            app.pane_title(Panel::Inbox),
            format!("{} — {}", tr!("folderstorm-inbox"), app.title)
        );
        asks(&mut app, 1, 7, AttentionKind::Inventory, "Kim", "a lantern", now);
        app.host.chat.attention_list(
            0,
            1,
            AttentionList {
                generation: 1,
                items: vec![AttentionItem {
                    id: 1,
                    kind: AttentionKind::Call,
                    title: "Sam".into(),
                    text: String::new(),
                }],
                more: 3,
            },
        );
        app.host.sync_inbox(now);
        draw_in(&mut app, Which::Pane(Panel::Inbox), 520, 380);
        let (drawn, pixels) = draw_in(&mut app, Which::Pane(Panel::Inbox), 520, 380);
        assert!(shows(&drawn, "Shop") && shows(&drawn, "— Kim: a lantern"), "{drawn:?}");
        assert!(shows(&drawn, &format!("{} — Sam", attention_kind(AttentionKind::Call))));
        assert!(
            shows(&drawn, &tr!("folderstorm-inbox-more", count = 3u32, name = "demo-1".to_owned())),
            "{drawn:?}"
        );
        let tint = faces::tint(&app.palette, Colour::Pink).expect("a colour");
        let pink = u32::from(tint.r()) << 16 | u32::from(tint.g()) << 8 | u32::from(tint.b());
        assert!(pixels.iter().any(|pixel| *pixel & 0x00FF_FFFF == pink), "the character's colour");
        assert!(painted(&pixels) > 3_000);
        assert!(
            app.pane_title(Panel::Inbox)
                .starts_with(&format!("{} (5)", tr!("folderstorm-inbox")))
        );
        // the host's window: no band (the inbox is elsewhere), the badge
        // stays, and the chat's "All" lists what waits as it always did
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, &format!("{} (5)", tr!("folderstorm-inbox"))), "{drawn:?}");
        assert!(shows(&drawn, "has something to answer"), "{drawn:?}");
        assert!(!app.chat.inbox_beside);
        // closing the inbox's window docks it, as any panel
        assert!(app.pane_event(Panel::Inbox, &winit::event::WindowEvent::CloseRequested));
        assert!(!app.out(Panel::Inbox) && !app.host.exit);
        draw(&mut app, 1120, 760);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "— Kim: a lantern"), "{drawn:?}");
    }

    #[test]
    fn dismissing_takes_questions_out_of_the_host_only() {
        let now = Instant::now();
        let mut app = window(host());
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
        asks(&mut app, 0, 3, AttentionKind::Script, "Door", "Open?", now);
        asks(&mut app, 1, 7, AttentionKind::Teleport, "Jane Doe", "Join me", now);
        asks(&mut app, 1, 8, AttentionKind::Friendship, "Kim", "Friends?", now);
        draw(&mut app, 1120, 760);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-inbox-dismiss-menu")), "{drawn:?}");
        assert!(
            drawn.iter().filter(|t| t.contains(tr!("folderstorm-inbox-dismiss"))).count() >= 4,
            "a Dismiss per row"
        );
        // one row: gone from the inbox and its counts, nothing for a viewer
        app.apply(vec![Act::InboxDismiss(1, 7)], now);
        assert_eq!(app.host.inbox.count(), 2);
        assert!(app.host.chat.take_effects().is_empty(), "nothing is said to a viewer");
        assert!(app.host.desk.take_effects().is_empty());
        // its viewer lists it still: it stays out
        app.host.sync_inbox(now);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(!shows(&drawn, "Join me") && shows(&drawn, "Friends?"), "{drawn:?}");
        assert!(shows(&drawn, &format!("{} (2)", tr!("folderstorm-inbox"))));
        // the chat's own box, beside no inbox, leaves it out too
        app.host.desk.set_panel_out(Panel::Inbox, true);
        app.panes[Panel::Inbox.index()] = Some(app.look.view());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "Friends?") && !shows(&drawn, "Join me"), "{drawn:?}");
        // all of one character's, then all
        app.apply(vec![Act::InboxDismissSlot(1)], now);
        assert_eq!((app.host.inbox.count_for(1), app.host.inbox.count()), (0, 1));
        app.apply(vec![Act::InboxDismissAll], now);
        assert!(app.host.inbox.is_empty());
        let (drawn, _) = draw_in(&mut app, Which::Pane(Panel::Inbox), 520, 380);
        assert!(shows(&drawn, tr!("folderstorm-inbox-empty")), "{drawn:?}");
        // a new question comes in as usual
        asks(&mut app, 1, 9, AttentionKind::Teleport, "Lee", "Come over", now);
        assert_eq!(app.host.inbox.count(), 1);
        assert!(app.host.chat.take_effects().is_empty());
    }

    #[test]
    fn a_removed_character_leaves_nothing_in_the_inbox() {
        let now = Instant::now();
        let mut app = window(host());
        asks(&mut app, 1, 7, AttentionKind::Teleport, "Jane Doe", "Join me", now);
        assert_eq!(app.host.inbox.count(), 1);
        app.apply(vec![Act::Remove(1)], now);
        assert!(app.host.inbox.is_empty());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(!shows(&drawn, "Join me"));
    }

    /// Every icon this file names is one the viewer embeds.
    #[test]
    fn the_inbox_s_icons_exist() {
        for name in ["bell-filled", "arrows-exchange", "bell-off"] {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
