//! Folderstorm: a managed viewer's side of the session host's shared chat
//! (session-host plan, section 9, checkpoint 3).
//!
//! - **Told to the host**: the conversations, read from the world a few
//!   times a second (`crate::folderstorm_characters::chat_export`). Nothing
//!   in Aurora's chat code is hooked.
//! - **Said for the host**: a line the user typed in the host's window for
//!   this character goes the way a line typed here goes: the chat bar's
//!   `App::send_local_chat` for the nearby chat (channels, gestures, chat
//!   bar commands, RLVa's redirections and refusals), and for a
//!   conversation the steps of the conversations window's send in
//!   `App::handle_actions` (":" emotes, emoji, a group's or conference's
//!   session, the unblock notice, `App::send` and its RLVa rules). What
//!   was said, or the notice of a refusal, then shows in the conversation
//!   and reaches the host from there like any line.
//! - A line is said only by the login it was typed for, in a conversation
//!   open here, and at most once.

use super::*;
use crate::folderstorm_characters::chat_export;
use crate::folderstorm_characters::managed::Link;
use ::folderstorm_session::ipc::{Conversation, SendOutcome, SendRefusal, ToHost, ToViewer};

/// The demo's characters hear something this often.
const DEMO_EVERY: Duration = Duration::from_secs(9);

impl App {
    /// A chat command of the host: a line to say, or a conversation read
    /// in the host's window.
    pub(super) fn folderstorm_chat_command(&mut self, command: ToViewer) {
        match command {
            ToViewer::SendChat {
                id,
                generation,
                conversation,
                text,
            } => {
                let outcome = self.folderstorm_host_line(id, generation, conversation, text);
                if let SendOutcome::Refused(why) = outcome {
                    log::info!("session host: line {id} not said ({why:?})");
                }
                self.folderstorm_characters.chat.answer(ToHost::SendResult { id, outcome });
            }
            ToViewer::MarkRead { generation, conversation } => {
                if !self.folderstorm_logged_in_as(generation) {
                    return;
                }
                if let Some(id) = conversation.id().map(uuid::Uuid::from_u128)
                    && let Some(session) = self.world.social.ims.iter_mut().find(|s| s.other == id)
                {
                    session.unread = 0;
                    self.world.acknowledge_read_messages();
                }
            }
            _ => {}
        }
    }

    /// In the world, as the login the host's command was made for.
    fn folderstorm_logged_in_as(&self, generation: u32) -> bool {
        matches!(self.screen, Screen::World)
            && self.folderstorm_characters.report.logged_in
            && self.folderstorm_characters.report.generation == generation
    }

    /// Says a line the user typed in the host's window, if it is this
    /// login's, new, and for a conversation open here.
    fn folderstorm_host_line(&mut self, id: u64, generation: u32, conversation: Conversation, text: String) -> SendOutcome {
        if !self.folderstorm_characters.chat.sends.fresh(id) {
            return SendOutcome::Refused(SendRefusal::Repeated);
        }
        if !matches!(self.screen, Screen::World) {
            return SendOutcome::Refused(SendRefusal::NotInWorld);
        }
        if !self.folderstorm_logged_in_as(generation) {
            return SendOutcome::Refused(SendRefusal::Stale);
        }
        // (cleaned by the link already; again here, where it matters)
        let text = ::folderstorm_session::ipc::clean_text(&text, ::folderstorm_session::ipc::MAX_SEND);
        if text.is_empty() {
            return SendOutcome::Refused(SendRefusal::Empty);
        }
        let Some(to) = conversation.id().map(uuid::Uuid::from_u128) else {
            // the chat bar's line: "/5 text" goes to its channel
            let (channel, message) = ui::chat::parse_channel(&text);
            self.send_local_chat(ui::chat::OutgoingChat {
                message,
                channel,
                chat_type: aurora_net::ChatType::Normal,
            });
            return SendOutcome::Accepted;
        };
        // only a conversation this viewer has open: the host starts none
        if !self.world.social.has_session(&to) {
            return SendOutcome::Refused(SendRefusal::Closed);
        }
        self.folderstorm_send_im(to, text);
        SendOutcome::Accepted
    }

    /// The conversations window's send (`App::handle_actions`, its loop
    /// over `a.ims`), step for step. Keep both alike at upstream merges.
    fn folderstorm_send_im(&mut self, to: uuid::Uuid, mut text: String) {
        // ":" as "/me" in the conversations too (FSFloaterIM::sendMsg)
        if self.settings.colon_emote {
            ui::chat::colon_emote(&mut text);
        }
        let text = ui::emoji::expand_shortcodes(&text);
        self.chat_ui.recent_emojis.note(&text);
        if self.world.is_group_session(&to) {
            self.world.send_session_im(to, &text);
            return;
        }
        if self.world.mutes.is_muted_id(&to) {
            let name = self.world.legacy_name(&to).unwrap_or_else(|| self.world.social.name_of(&to));
            self.world.unblock(to, &name, 0);
            self.world.system_notice(
                aurora_i18n::tr!("app-was-unblocked-because-you-sent-them-an-im", name = name.to_string()),
                crate::world::notice_topics::Topic::People,
            );
        }
        self.world.own_im(to, &text);
        self.send(NetCommand::SendIm { to, message: text });
    }

    /// Every turn of the event loop while managed: the answers that wait
    /// go to the host, and a few times a second the conversations are
    /// looked at. Never waits.
    pub(super) fn folderstorm_chat_pump(&mut self, link: &Link, now: Instant) {
        let mut export = std::mem::take(&mut self.folderstorm_characters.chat);
        export.flush(&mut |message| link.send(message));
        if export.checked.is_none_or(|at| now.duration_since(at) >= chat_export::EVERY) {
            export.checked = Some(now);
            let report = &self.folderstorm_characters.report;
            if matches!(self.screen, Screen::World) && report.logged_in {
                let generation = report.generation;
                if self.demo {
                    self.folderstorm_chat_demo(&mut export, now);
                }
                export.step(&self.world, generation, now, &mut |message| link.send(message));
            } else {
                export.out_of_world();
            }
        }
        self.folderstorm_characters.chat = export;
    }

    /// `AURORA_DEMO_SESSION_HOST`: a demo character hears a neighbour, gets
    /// an instant message, and once a teleport offer, so that the host's
    /// chat has something to show without a grid.
    fn folderstorm_chat_demo(&mut self, export: &mut chat_export::Export, now: Instant) {
        let (due, count) = export.demo.unwrap_or((now + DEMO_EVERY / 3, 0));
        if now < due {
            export.demo = Some((due, count));
            return;
        }
        export.demo = Some((now + DEMO_EVERY, count + 1));
        let agent = self.world.agent_id;
        let number = i64::from(count / 3 + 1);
        let events = match count % 3 {
            0 => vec![NetEvent::Chat(aurora_net::ChatMessage {
                from_name: "Nova Demo".into(),
                source_id: crate::demo::DEMO_NOVA,
                owner_id: crate::demo::DEMO_NOVA,
                source_type: aurora_net::ChatSourceType::Agent,
                chat_type: aurora_net::ChatType::Normal,
                position: glam::Vec3::new(130.0, 128.0, 30.0),
                message: aurora_i18n::tr!("folderstorm-chat-demo-nearby", n = number),
            })],
            1 => vec![NetEvent::InstantMessage(aurora_net::InstantMessage {
                from_agent_id: crate::demo::DEMO_TESS,
                from_name: "Tess Touch".into(),
                to_agent_id: agent,
                dialog: 0,
                session_id: uuid::Uuid::from_u128(agent.as_u128() ^ crate::demo::DEMO_TESS.as_u128()),
                message: aurora_i18n::tr!("folderstorm-chat-demo-im", n = number),
                offline: false,
                from_group: false,
                binary_bucket: Vec::new(),
                timestamp: 0,
            })],
            _ if count == 2 => crate::demo::lure_events(crate::demo::LureScene::Offer, agent),
            _ => Vec::new(),
        };
        for event in events {
            if let Some(event) = self.world.apply(event) {
                self.on_app_event(event);
            }
        }
    }
}
