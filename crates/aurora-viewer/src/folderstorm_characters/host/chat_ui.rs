//! The shared chat in the session host's window (session-host plan,
//! section 9): a tab per character and "All", the character's
//! conversations, what waits for an answer in its own window, and a box
//! that says, above what is typed, who will say it and where.
//!
//! Drawn like the rest of the host's window: egui on the processor
//! (`crash/software.rs`), no graphics device. The rules (whose draft this
//! is, what may be sent, what is uncertain) are the `folderstorm-session`
//! crate's `chat::Chat`; this file only shows it.

use super::Host;
use super::faces::{self, Face};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::chat::{Blocked, Chat, Entry, Note, Thread, Viewing};
use ::folderstorm_session::inbox::Inbox;
use ::folderstorm_session::ipc::{self, AttentionKind, Conversation, LineKind, MAX_SLOTS, SendRefusal, SessionKind};
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::time::{Duration, Instant, UNIX_EPOCH};

const BUTTON: Vec2 = Vec2::new(0.0, 26.0);
/// The room kept under the lines for the box and what it says.
const COMPOSE_ROOM: f32 = 132.0;

/// What the chat panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    All,
    Character(usize),
}

/// The chat panel's own state: nothing of it decides who says what.
#[derive(Debug, Default)]
pub(super) struct ChatPanel {
    tab: Tab,
    /// The conversation shown in each character's tab.
    shown: [Conversation; MAX_SLOTS],
    /// In "All": the conversation the box writes to, chosen by a click on
    /// one of its lines. Never chosen by what arrives.
    target: Option<(usize, Conversation)>,
    /// Checkpoint 7: the attention inbox is drawn in this same window, so
    /// "All" leaves what waits for an answer to it. Elsewhere (or
    /// nowhere), "All" lists it itself, as before.
    pub inbox_beside: bool,
}

/// What the panel asks of the window.
#[derive(Debug, Default)]
pub(super) struct Asked {
    /// Bring that character forward (to answer in its own window).
    pub switch: Option<usize>,
    /// Something was sent or changed: the host looks at once.
    pub acted: bool,
}

/// An unread count as the viewer's own counters write it.
pub(super) fn count_text(n: u32) -> String {
    if n > 99 { "99+".to_owned() } else { n.to_string() }
}

/// The name of a conversation's tab.
fn title(thread: &Thread) -> String {
    match thread.kind {
        SessionKind::Nearby => tr!("common-nearby-chat").to_owned(),
        _ if thread.title.is_empty() => tr!("folderstorm-chat-untitled").to_owned(),
        _ => thread.title.clone(),
    }
}

fn refusal(reason: SendRefusal) -> &'static str {
    match reason {
        SendRefusal::NotInWorld => tr!("folderstorm-chat-refused-not-in-world"),
        SendRefusal::Stale => tr!("folderstorm-chat-refused-stale"),
        SendRefusal::Closed => tr!("folderstorm-chat-refused-closed"),
        SendRefusal::Restricted => tr!("folderstorm-chat-refused-restricted"),
        SendRefusal::Empty => tr!("folderstorm-chat-refused-empty"),
        SendRefusal::Repeated => tr!("folderstorm-chat-refused-repeated"),
    }
}

pub(super) fn attention_kind(kind: AttentionKind) -> &'static str {
    match kind {
        AttentionKind::Teleport => tr!("folderstorm-chat-attention-teleport"),
        AttentionKind::Friendship => tr!("folderstorm-chat-attention-friendship"),
        AttentionKind::Inventory => tr!("folderstorm-chat-attention-inventory"),
        AttentionKind::Group => tr!("folderstorm-chat-attention-group"),
        AttentionKind::Script => tr!("folderstorm-chat-attention-script"),
        AttentionKind::Permissions => tr!("folderstorm-chat-attention-permissions"),
        AttentionKind::Link => tr!("folderstorm-chat-attention-link"),
        AttentionKind::Call => tr!("folderstorm-chat-attention-call"),
        AttentionKind::Other => tr!("folderstorm-chat-attention-other"),
    }
}

impl ChatPanel {
    /// What is on screen, for the unread counts.
    pub(super) fn viewing(&self, chat: &Chat) -> Viewing {
        match self.tab {
            Tab::All => Viewing::All,
            Tab::Character(slot) => {
                let shown = self.shown[slot];
                match chat.thread(slot, shown) {
                    Some(_) => Viewing::Thread(slot, shown),
                    None => Viewing::Nothing,
                }
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui, p: &Palette, host: &mut Host, now: Instant) -> Asked {
        let mut asked = Asked::default();
        // (the alias where the character has one; its colour beside it)
        let names: Vec<(usize, Face)> = host.desk.slots().map(|(slot, s)| (slot, faces::face(p, s))).collect();
        let face = |slot: usize| names.iter().find(|(s, _)| *s == slot).map(|(_, face)| face);
        let name = |slot: usize| face(slot).map(|face| face.name.clone()).unwrap_or_default();
        let tint = |slot: usize| face(slot).and_then(|face| face.tint);
        // a character removed from the desk has no tab
        if let Tab::Character(slot) = self.tab
            && !names.iter().any(|(s, _)| *s == slot)
        {
            self.tab = Tab::All;
        }
        if self.target.is_some_and(|(slot, _)| !names.iter().any(|(s, _)| *s == slot)) {
            self.target = None;
        }
        ui.label(RichText::new(tr!("folderstorm-chat-heading")).size(18.0).strong().color(p.ink));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let count = |n: u32| (n > 0).then(|| count_text(n));
            let all = host.chat.total_unread();
            if widgets::pill_tab(ui, p, "", tr!("folderstorm-chat-all"), count(all), true, self.tab == Tab::All, true).clicked() {
                self.tab = Tab::All;
            }
            for (slot, face) in &names {
                let unread = host.chat.unread(*slot);
                let selected = self.tab == Tab::Character(*slot);
                let mut tab = widgets::pill_tab(ui, p, "", &face.name, count(unread), true, selected, true);
                if let Some(tint) = face.tint {
                    faces::underline(ui, tab.rect, tint);
                }
                if let Some(real) = &face.real {
                    // the name behind the alias
                    tab = tab.on_hover_text(real);
                }
                if tab.clicked() {
                    self.tab = Tab::Character(*slot);
                }
            }
        });
        ui.add_space(4.0);
        let target = match self.tab {
            Tab::All => self.target,
            Tab::Character(slot) => {
                self.conversations(ui, p, &host.chat, slot);
                host.chat.thread(slot, self.shown[slot]).map(|t| (slot, t.conversation))
            }
        };
        self.attention(ui, p, &host.chat, &host.inbox, &name, &mut asked);
        let room = (ui.available_height() - COMPOSE_ROOM).max(80.0);
        egui::Frame::new()
            .fill(p.field)
            .inner_margin(10.0)
            .corner_radius(ui.visuals().window_corner_radius)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(("folderstorm-chat-lines", self.tab == Tab::All, target))
                    .auto_shrink([false, false])
                    .max_height(room - 20.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| match self.tab {
                        Tab::All => self.feed(ui, p, &host.chat, &name, &tint, &mut asked),
                        Tab::Character(slot) => match host.chat.thread(slot, self.shown[slot]) {
                            Some(thread) => {
                                if thread.trimmed {
                                    ui.label(RichText::new(tr!("folderstorm-chat-trimmed")).italics().color(p.muted));
                                }
                                if thread.earlier_login {
                                    ui.label(RichText::new(tr!("folderstorm-chat-earlier-login")).italics().color(p.muted));
                                }
                                for entry in thread.entries() {
                                    if entry_row(ui, p, entry, None) == Clicked::Answer {
                                        asked.switch = Some(slot);
                                    }
                                }
                            }
                            None => {
                                ui.label(RichText::new(tr!("folderstorm-chat-empty", name = name(slot))).color(p.muted));
                            }
                        },
                    });
            });
        ui.add_space(6.0);
        match target {
            Some((slot, conversation)) => {
                // who will say it: the real name too, behind an alias
                let said_as = face(slot).map(Face::full).unwrap_or_default();
                compose(ui, p, &mut host.chat, slot, conversation, &name(slot), &said_as, now, &mut asked);
            }
            None => {
                let hint = match self.tab {
                    Tab::All => tr!("folderstorm-chat-pick"),
                    Tab::Character(_) => tr!("folderstorm-chat-nothing-yet"),
                };
                ui.label(RichText::new(hint).color(p.muted));
            }
        }
        asked
    }

    /// The conversations of a character, the nearby chat first.
    fn conversations(&mut self, ui: &mut egui::Ui, p: &Palette, chat: &Chat, slot: usize) {
        let threads = chat.threads(slot);
        if chat.thread(slot, self.shown[slot]).is_none()
            && let Some(first) = threads.first()
        {
            self.shown[slot] = first.conversation;
        }
        ui.horizontal_wrapped(|ui| {
            for thread in threads {
                let mut label = title(thread);
                if thread.unread > 0 {
                    label = format!("{label} ({})", count_text(thread.unread));
                }
                let icon = match thread.kind {
                    SessionKind::Nearby => "message-circle",
                    SessionKind::Resident => "user",
                    SessionKind::Group | SessionKind::Conference => "users",
                };
                if widgets::filter_pill(ui, p, icon, &label, self.shown[slot] == thread.conversation).clicked() {
                    self.shown[slot] = thread.conversation;
                }
            }
        });
        ui.add_space(4.0);
    }

    /// What waits for an answer in the characters' own windows. Nothing is
    /// answered here: the button brings that window forward. What was
    /// dismissed from the inbox is left out here too.
    fn attention(&self, ui: &mut egui::Ui, p: &Palette, chat: &Chat, inbox: &Inbox, name: &dyn Fn(usize) -> String, asked: &mut Asked) {
        let slots: Vec<usize> = match self.tab {
            // (the inbox beside the chat lists them all, once)
            Tab::All if self.inbox_beside => Vec::new(),
            Tab::All => (0..MAX_SLOTS).collect(),
            Tab::Character(slot) => vec![slot],
        };
        for slot in slots {
            let (listed, _) = chat.attention(slot);
            let login = chat.login(slot);
            let items: Vec<&ipc::AttentionItem> = listed.iter().filter(|item| !inbox.is_dismissed(slot, login, item.id)).collect();
            // (what is not listed, as the inbox counts it)
            let more = inbox.more(slot);
            if items.is_empty() {
                continue;
            }
            egui::Frame::new()
                .fill(p.field)
                .inner_margin(8.0)
                .corner_radius(ui.visuals().window_corner_radius)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(tr!("folderstorm-chat-attention", name = name(slot)))
                                .strong()
                                .color(p.amber),
                        );
                        let answer = tr!("folderstorm-chat-switch-to-answer");
                        if widgets::action_button(ui, p, "arrows-exchange", answer, ActionKind::Primary, BUTTON)
                            .on_hover_text(tr!("folderstorm-chat-switch-to-answer-help"))
                            .clicked()
                        {
                            asked.switch = Some(slot);
                        }
                    });
                    for item in items.iter().take(4) {
                        let text = if item.text.is_empty() {
                            format!("{} — {}", attention_kind(item.kind), item.title)
                        } else {
                            format!("{} — {}: {}", attention_kind(item.kind), item.title, item.text)
                        };
                        ui.add(egui::Label::new(RichText::new(text).color(p.ink)).truncate());
                    }
                    let hidden = items.len().saturating_sub(4) as u32 + more;
                    if hidden > 0 {
                        ui.label(RichText::new(tr!("folderstorm-chat-attention-more", count = hidden)).color(p.muted));
                    }
                });
            ui.add_space(4.0);
        }
    }

    /// The "All" tab: every character's lines as they came, each with the
    /// character and the conversation it belongs to; a click on a line
    /// makes its conversation the one the box writes to.
    fn feed(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        chat: &Chat,
        name: &dyn Fn(usize) -> String,
        tint: &dyn Fn(usize) -> Option<egui::Color32>,
        asked: &mut Asked,
    ) {
        if chat.feed().len() == 0 {
            ui.label(RichText::new(tr!("folderstorm-chat-all-empty")).color(p.muted));
        }
        for item in chat.feed() {
            let place = match chat.thread(item.slot, item.conversation) {
                Some(thread) => format!("{} › {}", name(item.slot), title(thread)),
                None => name(item.slot),
            };
            let chosen = self.target == Some((item.slot, item.conversation));
            match entry_row(ui, p, &item.entry, Some((&place, chosen, tint(item.slot)))) {
                Clicked::Nothing => {}
                Clicked::Place => self.target = Some((item.slot, item.conversation)),
                Clicked::Answer => asked.switch = Some(item.slot),
            }
        }
    }
}

/// What a click on a line's row asks for.
#[derive(PartialEq, Eq)]
enum Clicked {
    Nothing,
    /// Its character and conversation label ("All" tab).
    Place,
    /// "Switch to answer" of an offer's card.
    Answer,
}

/// One entry, as one text that wraps as a whole. `place`: the character
/// and conversation label of the "All" tab, whether the box writes there
/// and the character's colour; a click on the row then chooses that
/// conversation.
fn entry_row(ui: &mut egui::Ui, p: &Palette, entry: &Entry, place: Option<(&str, bool, Option<egui::Color32>)>) -> Clicked {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::FontId::new(body.size * 0.85, body.family.clone());
    let part = |font: &egui::FontId, color: egui::Color32, italics: bool| egui::TextFormat {
        font_id: font.clone(),
        color,
        italics,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    if let Some((label, chosen, tint)) = place {
        let color = if chosen { p.violet_light } else { tint.unwrap_or(p.muted) };
        job.append(&format!("[{label}] "), 0.0, part(&small, color, false));
    }
    let mut card = false;
    match entry {
        Entry::Line(line) => {
            let time = UNIX_EPOCH + Duration::from_secs(line.time);
            job.append(&format!("{} ", crate::ui::chat::hhmm(time)), 0.0, part(&small, p.muted, false));
            let (name_color, text_color) = match line.kind {
                LineKind::Own => (p.violet_light, p.ink),
                LineKind::Said => (p.ink, p.ink),
                LineKind::Object | LineKind::System => (p.muted, p.muted),
                LineKind::Card => (p.amber, p.ink),
            };
            if line.emote {
                job.append(&line.text, 0.0, part(&body, text_color, true));
            } else {
                if !line.from.is_empty() {
                    job.append(&format!("{}: ", line.from), 0.0, part(&body, name_color, false));
                }
                job.append(&line.text, 0.0, part(&body, text_color, false));
            }
            card = line.kind == LineKind::Card;
        }
        Entry::Gap(missed) => job.append(&tr!("folderstorm-chat-gap", count = *missed), 0.0, part(&body, p.muted, true)),
        Entry::Note(Note::Refused { reason, text }) => {
            let note = tr!("folderstorm-chat-refused", why = refusal(*reason), text = text.clone());
            job.append(&note, 0.0, part(&body, p.amber, false));
        }
        Entry::Note(Note::Uncertain { text }) => {
            job.append(
                &tr!("folderstorm-chat-uncertain", text = text.clone()),
                0.0,
                part(&body, p.danger, false),
            );
        }
        Entry::Note(Note::NotSent { text }) => {
            job.append(
                &tr!("folderstorm-chat-not-sent", text = text.clone()),
                0.0,
                part(&body, p.amber, false),
            );
        }
    }
    let mut clicked = Clicked::Nothing;
    ui.horizontal_wrapped(|ui| {
        let mut row = egui::Label::new(job);
        if place.is_some() {
            row = row.sense(egui::Sense::click());
        }
        let row = ui.add(row);
        if place.is_some()
            && row
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tr!("folderstorm-chat-pick-help"))
                .clicked()
        {
            clicked = Clicked::Place;
        }
        // an offer's card: answered in the character's own window
        if card
            && widgets::action_button(
                ui,
                p,
                "arrows-exchange",
                tr!("folderstorm-chat-switch-to-answer"),
                ActionKind::Flat,
                Vec2::new(0.0, 22.0),
            )
            .on_hover_text(tr!("folderstorm-chat-switch-to-answer-help"))
            .clicked()
        {
            clicked = Clicked::Answer;
        }
    });
    clicked
}

/// The box: who will say what is typed, and where, above the field. The
/// draft shown is that character's in that conversation and no other's.
#[allow(clippy::too_many_arguments)]
fn compose(
    ui: &mut egui::Ui,
    p: &Palette,
    chat: &mut Chat,
    slot: usize,
    conversation: Conversation,
    name: &str,
    said_as: &str,
    now: Instant,
    asked: &mut Asked,
) {
    let place = chat.thread(slot, conversation).map(title).unwrap_or_default();
    ui.label(
        RichText::new(tr!("folderstorm-chat-send-as", name = said_as.to_owned(), place = place))
            .strong()
            .color(p.violet_light),
    );
    if chat.draft_held(slot, conversation) {
        ui.label(RichText::new(tr!("folderstorm-chat-held", name = name.to_owned())).color(p.amber));
        ui.add(egui::Label::new(RichText::new(chat.draft(slot, conversation)).color(p.ink)).truncate());
        ui.horizontal_wrapped(|ui| {
            let take = tr!("folderstorm-chat-held-take", name = name.to_owned());
            if widgets::action_button_if(ui, p, "check", &take, ActionKind::Flat, BUTTON, chat.logged_in(slot)).clicked() {
                asked.acted |= chat.adopt_draft(slot, conversation);
            }
            if widgets::action_button(
                ui,
                p,
                "trash-filled",
                tr!("folderstorm-chat-held-discard"),
                ActionKind::Flat,
                BUTTON,
            )
            .clicked()
            {
                chat.discard_draft(slot, conversation);
                asked.acted = true;
            }
        });
        return;
    }
    let state = chat.can_send(slot, conversation);
    let editable = chat.logged_in(slot);
    let mut text = chat.draft(slot, conversation).to_owned();
    let mut send = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 110.0).max(120.0);
        let field = widgets::text_field(ui, editable, &mut text, |b, _| {
            egui::TextEdit::singleline(b)
                .id_salt(("folderstorm-chat-draft", slot, conversation))
                .char_limit(ipc::MAX_SEND)
                .hint_text(tr!("folderstorm-chat-hint", name = name.to_owned()))
                .desired_width(width)
        });
        if field.changed() {
            chat.set_draft(slot, conversation, &text);
        }
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let can = chat.can_send(slot, conversation).is_ok();
        let button = widgets::action_button_if(ui, p, "send", tr!("common-send"), ActionKind::Primary, BUTTON, can);
        if (button.clicked() || entered) && can {
            send = true;
            // the next line is typed without another click
            field.request_focus();
        }
    });
    if send && chat.submit(slot, conversation, now).is_ok() {
        asked.acted = true;
    }
    let note = match state {
        Err(Blocked::NotInWorld) => Some(tr!("folderstorm-chat-blocked-not-in-world", name = name.to_owned())),
        Err(Blocked::EarlierLogin) => Some(tr!("folderstorm-chat-earlier-login").to_owned()),
        Err(Blocked::Busy) => Some(tr!("folderstorm-chat-blocked-busy", name = name.to_owned())),
        Err(Blocked::Held | Blocked::Empty) | Ok(()) => None,
    };
    if let Some(note) = note {
        ui.label(RichText::new(note).color(p.muted));
    } else if chat.sending(slot, conversation) > 0 {
        ui.label(RichText::new(tr!("folderstorm-chat-sending", name = name.to_owned())).color(p.muted));
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use ::folderstorm_session::host::{Config, Desk};
    use ::folderstorm_session::ipc::{AttentionItem, AttentionList, ChatBatch, ChatLine};

    const JANE: Conversation = Conversation::Session(0, 0x4a);

    /// A host with two characters in the world (`demo-1`, `demo-2`), no
    /// viewer started.
    pub(in super::super) fn host() -> Host {
        let (events_tx, events) = crossbeam_channel::unbounded();
        let mut host = Host {
            desk: Desk::new(Config::demo(2)),
            chat: Chat::default(),
            alert: false,
            comfort: Default::default(),
            viewers: Default::default(),
            screens: Default::default(),
            inbox: Default::default(),
            sample_inbox: false,
            events,
            events_tx,
            demo: true,
            unsaved: None,
            exit: false,
            bar_sent: Default::default(),
            bar_asked: Vec::new(),
        };
        for slot in 0..2 {
            host.chat.status(slot, 1, 1, true);
        }
        host
    }

    pub(in super::super) fn heard(
        host: &mut Host,
        slot: usize,
        conversation: Conversation,
        number: u64,
        kind: LineKind,
        from: &str,
        text: &str,
    ) {
        let resident = conversation != Conversation::Nearby;
        host.chat.batch(
            slot,
            1,
            ChatBatch {
                generation: 1,
                conversation,
                kind: if resident { SessionKind::Resident } else { SessionKind::Nearby },
                title: if resident { "Jane Doe".into() } else { String::new() },
                lines: vec![ChatLine {
                    number,
                    time: 0,
                    kind,
                    emote: false,
                    from: from.into(),
                    text: text.into(),
                }],
                ..Default::default()
            },
        );
    }

    /// One frame of the panel in a window `width` points wide; every text
    /// it drew, and what it asked for.
    fn frame(ctx: &egui::Context, panel: &mut ChatPanel, host: &mut Host, width: f32) -> (Vec<String>, Asked) {
        let p = crate::theme::Theme::default().palette();
        let mut asked = Asked::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 700.0))),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    asked = panel.show(ui, &p, host, Instant::now());
                });
            },
        );
        fn texts(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, into)),
                _ => {}
            }
        }
        let mut drawn = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut drawn);
        }
        output.textures_delta.clear();
        (drawn, asked)
    }

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    #[test]
    fn a_character_s_tab_shows_its_lines_and_says_who_will_speak() {
        let (ctx, mut panel, mut host) = (egui::Context::default(), ChatPanel::default(), host());
        heard(
            &mut host,
            0,
            Conversation::Nearby,
            1,
            LineKind::Said,
            "Jane Doe",
            "hello neighbours",
        );
        heard(&mut host, 1, JANE, 1, LineKind::Said, "Jane Doe", "a word for the second one");
        panel.tab = Tab::Character(0);
        for width in [1100.0, 420.0] {
            let (drawn, _) = frame(&ctx, &mut panel, &mut host, width);
            assert!(shows(&drawn, "hello neighbours"), "{drawn:?}");
            assert!(shows(&drawn, "Jane Doe:"));
            assert!(
                !shows(&drawn, "a word for the second one"),
                "another character's line is in its own tab"
            );
            assert!(shows(&drawn, "Send as demo-1"), "{drawn:?}");
        }
        assert_eq!(panel.viewing(&host.chat), Viewing::Thread(0, Conversation::Nearby));
        // the other character's tab: its own conversation, its own box
        assert!(host.chat.set_draft(0, Conversation::Nearby, "typed as the first"));
        panel.tab = Tab::Character(1);
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "a word for the second one"));
        assert!(shows(&drawn, "Send as demo-2"));
        assert!(!shows(&drawn, "typed as the first"), "{drawn:?}");
        assert!(!shows(&drawn, "Send as demo-1"));
    }

    #[test]
    fn all_shows_every_line_with_its_place_and_writes_nowhere_until_one_is_chosen() {
        let (ctx, mut panel, mut host) = (egui::Context::default(), ChatPanel::default(), host());
        heard(
            &mut host,
            0,
            Conversation::Nearby,
            1,
            LineKind::Said,
            "Jane Doe",
            "hello neighbours",
        );
        heard(&mut host, 1, JANE, 1, LineKind::Said, "Jane Doe", "a private word");
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "[demo-1 › Nearby chat]"), "{drawn:?}");
        assert!(shows(&drawn, "[demo-2 › Jane Doe]"));
        assert!(!shows(&drawn, "Send as"), "no conversation chosen: no box");
        // a line that arrives does not choose where the box writes
        heard(&mut host, 0, JANE, 1, LineKind::Said, "Jane Doe", "newest line");
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(!shows(&drawn, "Send as"));
        panel.target = Some((1, JANE));
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Send as demo-2 — Jane Doe"), "{drawn:?}");
        heard(&mut host, 0, JANE, 2, LineKind::Said, "Jane Doe", "to the other character");
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Send as demo-2 — Jane Doe"), "the box stays where the user put it");
    }

    #[test]
    fn offers_are_only_listed_and_a_held_draft_is_not_typed_into() {
        let (ctx, mut panel, mut host) = (egui::Context::default(), ChatPanel::default(), host());
        heard(&mut host, 0, JANE, 1, LineKind::Card, "Jane Doe", "offers a teleport");
        host.chat.attention_list(
            0,
            1,
            AttentionList {
                generation: 1,
                items: vec![AttentionItem {
                    id: 1,
                    kind: AttentionKind::Teleport,
                    title: "Jane Doe".into(),
                    text: "Join me".into(),
                }],
                more: 0,
            },
        );
        panel.tab = Tab::Character(0);
        let (drawn, asked) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Attention: demo-1 has something to answer"), "{drawn:?}");
        assert!(shows(&drawn, "Teleport — Jane Doe: Join me"));
        assert!(shows(&drawn, "Switch to answer"));
        assert_eq!(asked.switch, None, "nothing is answered or switched without a click");
        assert!(host.chat.take_effects().iter().all(|e| !matches!(
            e,
            ::folderstorm_session::chat::Effect::Send {
                message: ipc::ToViewer::SendChat { .. },
                ..
            }
        )));
        // the character's viewer is restarted while a text waits
        assert!(host.chat.set_draft(0, JANE, "typed before the restart"));
        host.chat.ended(0, 1);
        host.chat.status(0, 2, 1, true);
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "typed before the restart"));
        assert!(shows(&drawn, "It is not sent unless you take it up again"), "{drawn:?}");
        assert!(shows(&drawn, "Use it as demo-1 now"));
        assert!(!shows(&drawn, "Type as demo-1"), "no field while the draft is held");
    }

    #[test]
    fn an_alias_names_the_tab_and_the_place_and_the_real_name_is_said_where_a_line_is_sent() {
        let (ctx, mut panel, mut host) = (egui::Context::default(), ChatPanel::default(), host());
        assert_eq!(host.desk.set_alias(0, "Main"), Ok(()));
        host.desk.set_colour(0, ::folderstorm_session::identity::Colour::Green);
        heard(
            &mut host,
            0,
            Conversation::Nearby,
            1,
            LineKind::Said,
            "Jane Doe",
            "hello neighbours",
        );
        heard(&mut host, 1, JANE, 1, LineKind::Said, "Jane Doe", "a private word");
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "[Main › Nearby chat]"), "{drawn:?}");
        assert!(shows(&drawn, "[demo-2 › Jane Doe]"));
        assert!(!shows(&drawn, "[demo-1"), "the alias stands for the name");
        // who will say a line: the alias and, with it, the real name
        panel.tab = Tab::Character(0);
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Send as Main (demo-1)"), "{drawn:?}");
        // a character without an alias is said as before
        panel.tab = Tab::Character(1);
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Send as demo-2 — Jane Doe"), "{drawn:?}");
    }

    #[test]
    fn beside_the_inbox_all_leaves_what_waits_for_an_answer_to_it() {
        let (ctx, mut panel, mut host) = (egui::Context::default(), ChatPanel::default(), host());
        host.chat.attention_list(
            0,
            1,
            AttentionList {
                generation: 1,
                items: vec![AttentionItem {
                    id: 1,
                    kind: AttentionKind::Teleport,
                    title: "Jane Doe".into(),
                    text: "Join me".into(),
                }],
                more: 0,
            },
        );
        // no inbox in this window: "All" lists it, as it always did
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Attention: demo-1 has something to answer"), "{drawn:?}");
        // the inbox is drawn in this window: once is enough
        panel.inbox_beside = true;
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(!shows(&drawn, "has something to answer"), "{drawn:?}");
        // the character's own tab keeps its own list either way
        panel.tab = Tab::Character(0);
        let (drawn, _) = frame(&ctx, &mut panel, &mut host, 1100.0);
        assert!(shows(&drawn, "Teleport — Jane Doe: Join me"), "{drawn:?}");
    }
}
