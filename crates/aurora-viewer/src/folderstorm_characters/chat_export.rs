//! What a managed viewer tells the session host of its conversations
//! (session-host plan, section 9). **By observation**: nothing in Aurora's
//! chat code calls this. A few times a second [`Export::step`] reads the
//! nearby chat (`World::chat`) and the conversations
//! (`World::social.ims`: residents, groups and conferences alike), and the
//! `LineClock` of each says which lines are new
//! (`folderstorm_session::export`).
//!
//! **Only what this viewer's own window shows leaves it.** A line is told
//! with the name and the text the conversation shows for it now
//! (`ui::chat::sender_name`, `line_body`, `resolved_text`): the name a
//! restriction replaces is replaced, the place it hides is hidden, a line
//! RLVa refused at reception is not in the list at all. While RLVa is on,
//! the lines already told are checked again against what the window would
//! show now; when one differs (a restriction came, a hidden resident's
//! name was learnt), the conversation is told again from its last lines
//! and the host replaces what it held.
//!
//! Told: the lines of this session only (`Origin::Live`). The history read
//! from the transcripts stays in the viewer: the host keeps no archive.

use crate::ui::chat::{line_body, resolved_text, sender_name};
use crate::world::notifications::Kind;
use crate::world::transcripts::LineClock;
use crate::world::{ChatKind, ChatLine, Origin, World};
use ::folderstorm_session::export::{Clock, Cursor, Sends};
use ::folderstorm_session::ipc::{
    self, AttentionItem, AttentionKind, AttentionList, ChatBatch, Conversation, LineKind, SessionKind, ToHost,
};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant, UNIX_EPOCH};
use uuid::Uuid;

/// The conversations are looked at this often.
pub const EVERY: Duration = Duration::from_millis(250);
/// While RLVa is on, the lines told are checked again this often.
const RECHECK_EVERY: Duration = Duration::from_secs(2);
/// How many of a conversation's last told lines are checked again.
const RECHECK_LINES: usize = 200;
/// Answers to the host that wait for room in the link's queue.
const MAX_OUTBOX: usize = 64;

/// Where the export of one conversation is.
#[derive(Debug, Default)]
struct Told {
    cursor: Cursor,
    /// The title and unread count last told.
    title: String,
    unread: u32,
    /// The last lines told: their numbers, and what was told of them.
    lines: VecDeque<(u64, u64)>,
}

/// What a managed viewer keeps of its chat for the host.
#[derive(Debug, Default)]
pub struct Export {
    /// The login the conversations are told for.
    generation: Option<u32>,
    /// The lines the host asked this viewer to say, each at most once.
    pub sends: Sends,
    nearby: Told,
    sessions: HashMap<Uuid, Told>,
    /// Answers to the host not queued yet (the queue was full).
    outbox: VecDeque<ToHost>,
    pub checked: Option<Instant>,
    rechecked: Option<Instant>,
    /// RLVa on, names hidden, places hidden, as of the last look.
    privacy: (bool, bool, bool),
    attention: Option<AttentionList>,
    /// The demo's next line, and how many it made.
    pub demo: Option<(Instant, u32)>,
}

fn clock_of(clock: LineClock, len: usize) -> Clock {
    Clock {
        first: clock.first,
        revision: clock.revision,
        token: clock.token,
        len,
    }
}

/// A line as the host is told it: what the conversation shows of it now.
/// None for a line that is not told (history, an IM repeated in the
/// nearby chat).
pub fn wire_line(world: &World, own_name: &str, number: u64, line: &ChatLine, nearby: bool) -> Option<ipc::ChatLine> {
    if line.origin != Origin::Live || (nearby && line.kind == ChatKind::Im) {
        return None;
    }
    let kind = match line.kind {
        ChatKind::Local(_) | ChatKind::Im => LineKind::Said,
        ChatKind::Own(_) => LineKind::Own,
        ChatKind::System | ChatKind::Payment(_) => LineKind::System,
        ChatKind::Object(_) | ChatKind::ObjectIm => LineKind::Object,
        // answered in this viewer's window, never in the host
        ChatKind::Invite(_) | ChatKind::InventoryOffer(_) => LineKind::Card,
    };
    let (from, body, emote) = match line.kind {
        ChatKind::System | ChatKind::Payment(_) => (String::new(), line.text.clone(), false),
        ChatKind::Invite(_) | ChatKind::InventoryOffer(_) => (sender_name(world, own_name, line), line.text.clone(), false),
        _ => {
            let name = sender_name(world, own_name, line);
            let (body, emote) = line_body(line, &name);
            (name, body, emote)
        }
    };
    Some(ipc::ChatLine {
        number,
        time: line.time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        kind,
        emote,
        from,
        text: resolved_text(world, &body),
    })
}

fn fingerprint(line: &ipc::ChatLine) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (&line.from, &line.text, line.emote).hash(&mut hasher);
    hasher.finish()
}

/// What a conversation is and the name its tab shows.
fn describe(world: &World, id: &Uuid) -> (SessionKind, String) {
    match world.groups.sessions.get(id) {
        Some(info) => {
            let kind = if info.group { SessionKind::Group } else { SessionKind::Conference };
            (kind, world.rlva_filter_text(&world.session_title(id)).into_owned())
        }
        None if world.groups.group(id).is_some() => (SessionKind::Group, world.rlva_filter_text(&world.session_title(id)).into_owned()),
        None if !world.rlva_can_name(*id) => (
            SessionKind::Resident,
            crate::world::rlva::privacy::anonymous(*id).display_name.clone(),
        ),
        None => (SessionKind::Resident, world.rlva_filter_text(&world.session_title(id)).into_owned()),
    }
}

/// What waits for the user's answer in this viewer, as its notification
/// list words it; `generation` is filled in by the caller.
pub fn attention(world: &World) -> AttentionList {
    with_question(world, crate::mcp_bridge::open_question())
}

/// The number an assistant's question takes in the list: far from the
/// viewer's own notifications' numbers.
const ASSISTANT_ID: u64 = 1 << 62;

/// [`attention`], with the local assistant's open question first, if any
/// (session routing: folderstorm/plans/mcp-routing.md, section 4).
fn with_question(world: &World, question: Option<crate::mcp_bridge::desk::Question>) -> AttentionList {
    let waiting: Vec<_> = world.notifications.listed().filter(|n| n.interactive()).collect();
    let asked = question.map(|q| AttentionItem {
        id: ASSISTANT_ID | (q.number & (ASSISTANT_ID - 1)),
        kind: AttentionKind::Assistant,
        title: q.client.unwrap_or_else(|| aurora_i18n::tr!("fs-mcp-ask-title").to_owned()),
        text: q.summary,
    });
    let room = ipc::MAX_ATTENTION - usize::from(asked.is_some());
    let items = asked
        .into_iter()
        .chain(waiting.iter().take(room).map(|n| AttentionItem {
            id: n.id,
            kind: match n.kind {
                Kind::Teleport => AttentionKind::Teleport,
                Kind::Friendship => AttentionKind::Friendship,
                Kind::Inventory => AttentionKind::Inventory,
                Kind::Group => AttentionKind::Group,
                Kind::Script => AttentionKind::Script,
                Kind::Permissions => AttentionKind::Permissions,
                Kind::Url => AttentionKind::Link,
                Kind::Call => AttentionKind::Call,
                Kind::Info | Kind::Alert | Kind::Presence | Kind::Message => AttentionKind::Other,
            },
            title: resolved_text(world, &n.title),
            text: resolved_text(world, &n.body),
        }))
        .collect();
    AttentionList {
        generation: 0,
        items,
        more: u32::try_from(waiting.len().saturating_sub(room)).unwrap_or(u32::MAX),
    }
    .cleaned()
}

/// One conversation as [`Told::tell`] reads it.
struct Source<'a> {
    conversation: Conversation,
    kind: SessionKind,
    title: String,
    unread: u32,
    lines: &'a VecDeque<ChatLine>,
    clock: Clock,
}

impl Told {
    /// Tells what is new of one conversation. False when the link took no
    /// more: the rest waits for the next look, nothing is skipped.
    fn tell(&mut self, world: &World, own_name: &str, generation: u32, source: &Source<'_>, send: &mut dyn FnMut(ToHost) -> bool) -> bool {
        let nearby = source.conversation == Conversation::Nearby;
        loop {
            let Some(plan) = self.cursor.plan(source.clock) else {
                // nothing new but its name or its unread count
                if self.cursor.started() && (self.title != source.title || self.unread != source.unread) {
                    let batch = ChatBatch {
                        generation,
                        conversation: source.conversation,
                        kind: source.kind,
                        title: source.title.clone(),
                        unread: source.unread,
                        ..Default::default()
                    };
                    if !send(ToHost::Chat(Box::new(batch))) {
                        return false;
                    }
                    self.title.clone_from(&source.title);
                    self.unread = source.unread;
                }
                return true;
            };
            let mut batch = ChatBatch {
                generation,
                conversation: source.conversation,
                kind: source.kind,
                title: source.title.clone(),
                restart: plan.restart,
                missed: u32::try_from(plan.missed).unwrap_or(u32::MAX),
                unread: source.unread,
                lines: Vec::new(),
            };
            let mut end = plan.lines.start;
            let mut told = Vec::new();
            for i in plan.lines.clone() {
                let number = source.clock.first + i as u64;
                if let Some(line) = source.lines.get(i).and_then(|l| wire_line(world, own_name, number, l, nearby)) {
                    let print = fingerprint(&line);
                    if !batch.push(line) {
                        // full: this line opens the next batch
                        break;
                    }
                    told.push((number, print));
                }
                end = i + 1;
            }
            if !send(ToHost::Chat(Box::new(batch))) {
                return false;
            }
            if plan.restart {
                self.lines.clear();
            }
            self.lines.extend(told);
            while self.lines.len() > RECHECK_LINES {
                self.lines.pop_front();
            }
            self.cursor.told(source.clock, end);
            self.title.clone_from(&source.title);
            self.unread = source.unread;
            if end >= source.clock.len {
                return true;
            }
        }
    }

    /// Whether the lines told still read as the window shows them now;
    /// if one does not, the conversation is told again.
    fn recheck(&mut self, world: &World, own_name: &str, source: &Source<'_>) {
        let nearby = source.conversation == Conversation::Nearby;
        let changed = self.lines.iter().any(|(number, print)| {
            number
                .checked_sub(source.clock.first)
                .and_then(|i| source.lines.get(i as usize))
                .is_some_and(|line| wire_line(world, own_name, *number, line, nearby).map(|l| fingerprint(&l)) != Some(*print))
        });
        if changed {
            self.cursor.restart();
        }
    }
}

impl Export {
    /// An answer for the host (what became of a line it asked for): kept
    /// until the link's queue takes it.
    pub fn answer(&mut self, message: ToHost) {
        if self.outbox.len() < MAX_OUTBOX {
            self.outbox.push_back(message);
        }
        // (over the bound the host hears nothing, and calls the line
        // uncertain: it is never said twice either way)
    }

    /// Hands the waiting answers to the link, in order.
    pub fn flush(&mut self, send: &mut dyn FnMut(ToHost) -> bool) {
        while let Some(message) = self.outbox.pop_front() {
            if !send(message.clone()) {
                self.outbox.push_front(message);
                return;
            }
        }
    }

    /// The viewer is not in the world: nothing is told, and what was told
    /// belongs to a login that is over.
    pub fn out_of_world(&mut self) {
        if self.generation.take().is_some() {
            self.forget();
        }
    }

    fn forget(&mut self) {
        self.nearby = Told::default();
        self.sessions.clear();
        self.attention = None;
        self.rechecked = None;
        self.demo = None;
    }

    /// Looks at the world and tells the host what is new, for the login
    /// `generation`. `send` queues one message for the host and says
    /// whether it was taken; what is not taken is told at the next look.
    pub fn step(&mut self, world: &World, generation: u32, now: Instant, send: &mut dyn FnMut(ToHost) -> bool) {
        if self.generation != Some(generation) {
            self.forget();
            self.generation = Some(generation);
        }
        let own_name = world.own_name();
        let privacy = (world.rlva.enabled(), world.rlva.has("shownames"), world.rlva.has("showloc"));
        let mut recheck = false;
        if std::mem::replace(&mut self.privacy, privacy) != privacy {
            // what the window shows of every line may have changed
            self.nearby.cursor.restart();
            for told in self.sessions.values_mut() {
                told.cursor.restart();
            }
        } else if privacy.0 && self.rechecked.is_none_or(|at| now.duration_since(at) >= RECHECK_EVERY) {
            self.rechecked = Some(now);
            recheck = true;
        }
        let nearby = Source {
            conversation: Conversation::Nearby,
            kind: SessionKind::Nearby,
            title: String::new(),
            unread: 0,
            lines: &world.chat,
            clock: clock_of(world.chat_clock, world.chat.len()),
        };
        if recheck {
            self.nearby.recheck(world, &own_name, &nearby);
        }
        if !self.nearby.tell(world, &own_name, generation, &nearby, send) {
            return;
        }
        self.sessions.retain(|id, _| world.social.ims.iter().any(|s| s.other == *id));
        for session in &world.social.ims {
            let (kind, title) = describe(world, &session.other);
            let source = Source {
                conversation: Conversation::session(session.other.as_u128()),
                kind,
                title,
                unread: u32::try_from(session.unread).unwrap_or(u32::MAX),
                lines: &session.lines,
                clock: clock_of(session.clock, session.lines.len()),
            };
            let told = self.sessions.entry(session.other).or_default();
            if recheck {
                told.recheck(world, &own_name, &source);
            }
            if !told.tell(world, &own_name, generation, &source, send) {
                return;
            }
        }
        let list = AttentionList {
            generation,
            ..attention(world)
        };
        if self.attention.as_ref() != Some(&list) && send(ToHost::Attention(Box::new(list.clone()))) {
            self.attention = Some(list);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_net::{ChatMessage, ChatSourceType, ChatType, InstantMessage, NetEvent};

    const ME: Uuid = Uuid::from_u128(99);
    const JANE: Uuid = Uuid::from_u128(0x4a41_4e45);
    const CONTROLLER: Uuid = Uuid::from_u128(0x726c_7661);

    fn world() -> World {
        let mut world = World::new(std::sync::Arc::new(crate::scene::avatar::AvatarLibrary::load()));
        world.agent_id = ME;
        world
    }

    fn say(world: &mut World, source: Uuid, name: &str, text: &str) {
        world.apply(NetEvent::Chat(ChatMessage {
            from_name: name.into(),
            source_id: source,
            owner_id: source,
            source_type: ChatSourceType::Agent,
            chat_type: ChatType::Normal,
            position: glam::Vec3::ZERO,
            message: text.into(),
        }));
    }

    fn im(world: &mut World, text: &str) {
        world.apply(NetEvent::InstantMessage(InstantMessage {
            from_agent_id: JANE,
            from_name: "Jane Doe".into(),
            to_agent_id: ME,
            dialog: 0,
            session_id: Uuid::from_u128(JANE.as_u128() ^ ME.as_u128()),
            message: text.into(),
            offline: false,
            from_group: false,
            binary_bucket: Vec::new(),
            timestamp: 0,
        }));
    }

    fn restrict(world: &mut World, commands: &str) {
        world.rlva.configure(true);
        world.apply(NetEvent::Chat(ChatMessage {
            from_name: "Controller".into(),
            source_id: CONTROLLER,
            owner_id: ME,
            source_type: ChatSourceType::Object,
            chat_type: ChatType::OwnerSay,
            position: glam::Vec3::ZERO,
            message: commands.into(),
        }));
    }

    /// One look with a link that takes everything; the batches told.
    fn look(export: &mut Export, world: &World, at: Instant) -> Vec<ChatBatch> {
        look_through(export, world, at, usize::MAX)
    }

    /// One look with a link that takes `room` messages.
    fn look_through(export: &mut Export, world: &World, at: Instant, mut room: usize) -> Vec<ChatBatch> {
        let mut told = Vec::new();
        export.step(world, 1, at, &mut |message| {
            if room == 0 {
                return false;
            }
            room -= 1;
            if let ToHost::Chat(batch) = message {
                told.push(*batch);
            }
            true
        });
        told
    }

    fn texts(batches: &[ChatBatch], conversation: Conversation) -> Vec<String> {
        batches
            .iter()
            .filter(|b| b.conversation == conversation)
            .flat_map(|b| b.lines.iter().map(|l| format!("{}: {}", l.from, l.text)))
            .collect()
    }

    fn jane() -> Conversation {
        Conversation::session(JANE.as_u128())
    }

    #[test]
    fn new_lines_are_told_once_as_the_window_shows_them() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        assert!(look(&mut export, &w, now).is_empty(), "nothing said, nothing told");
        say(&mut w, JANE, "Jane Doe", "hello there");
        im(&mut w, "a private word");
        let told = look(&mut export, &w, now);
        assert_eq!(texts(&told, Conversation::Nearby), ["Jane Doe: hello there"]);
        assert_eq!(texts(&told, jane()), ["Jane Doe: a private word"]);
        let private = told.iter().find(|b| b.conversation == jane()).expect("batch");
        assert_eq!((private.kind, private.restart, private.unread), (SessionKind::Resident, true, 1));
        assert_eq!(private.lines[0].kind, LineKind::Said);
        // nothing new: nothing told
        assert!(look(&mut export, &w, now).is_empty());
        say(&mut w, JANE, "Jane Doe", "/me waves");
        let told = look(&mut export, &w, now);
        assert_eq!(texts(&told, Conversation::Nearby), ["Jane Doe: Jane Doe waves"]);
        assert!(told[0].lines[0].emote && !told[0].restart);
        // our own line and a viewer's notice
        say(&mut w, ME, "Me Resident", "hi Jane");
        w.system_message("A notice");
        let told = look(&mut export, &w, now);
        let kinds: Vec<_> = told[0].lines.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, [LineKind::Own, LineKind::System]);
        assert_eq!(told[0].lines[1].from, "");
    }

    #[test]
    fn a_cleared_chat_starts_again_and_history_is_never_told() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        say(&mut w, JANE, "Jane Doe", "one");
        look(&mut export, &w, now);
        // the "clear" command: the clock's token changes
        w.clear_nearby_chat();
        let told = look(&mut export, &w, now);
        assert_eq!(told.len(), 1);
        assert!(told[0].restart && told[0].lines.is_empty(), "the host empties its copy");
        // lines read from a transcript are not told
        w.chat.push_back(ChatLine {
            time: std::time::SystemTime::now(),
            from: "Old Friend".into(),
            text: "from last week".into(),
            kind: ChatKind::Local(ChatType::Normal),
            source: Uuid::nil(),
            origin: Origin::Log,
        });
        w.chat_clock.pushed_back();
        say(&mut w, JANE, "Jane Doe", "two");
        let told = look(&mut export, &w, now);
        assert_eq!(texts(&told, Conversation::Nearby), ["Jane Doe: two"]);
    }

    #[test]
    fn a_full_link_delays_lines_and_loses_none() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        for n in 0..(ipc::MAX_BATCH_LINES + 5) {
            say(&mut w, JANE, "Jane Doe", &format!("line {n}"));
        }
        im(&mut w, "meanwhile");
        // the link takes one message: the first batch of the nearby chat
        let first = look_through(&mut export, &w, now, 1);
        assert_eq!(texts(&first, Conversation::Nearby).len(), ipc::MAX_BATCH_LINES);
        assert!(look_through(&mut export, &w, now, 0).is_empty());
        // then everything: the rest of the nearby chat, then the IM
        let rest = look(&mut export, &w, now);
        let nearby = texts(&rest, Conversation::Nearby);
        assert_eq!(nearby.len(), 5);
        assert_eq!(nearby.last().map(String::as_str), Some("Jane Doe: line 36"));
        assert!(rest.iter().filter(|b| b.conversation == Conversation::Nearby).all(|b| !b.restart));
        assert_eq!(texts(&rest, jane()), ["Jane Doe: meanwhile"]);
        assert!(look(&mut export, &w, now).is_empty());
    }

    #[test]
    fn what_rlva_refuses_at_reception_never_reaches_the_host() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        restrict(&mut w, "@recvchat=n,recvim=n");
        say(&mut w, JANE, "Jane Doe", "you cannot hear this");
        im(&mut w, "nor read this");
        let told = look(&mut export, &w, now);
        let all = format!("{told:?}");
        assert!(!all.contains("cannot hear") && !all.contains("nor read"), "{all}");
        // the command itself is not a line either
        assert!(!all.contains("@recvchat"), "{all}");
    }

    #[test]
    fn hidden_names_and_places_are_hidden_from_the_host_too() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        w.social.names.insert(JANE, "Jane Doe".into());
        say(&mut w, JANE, "Jane Doe", "it is me, Jane Doe");
        im(&mut w, "Jane Doe here");
        let told = look(&mut export, &w, now);
        assert_eq!(texts(&told, Conversation::Nearby), ["Jane Doe: it is me, Jane Doe"]);
        // the restriction comes after the lines were told: the host's copy
        // is replaced by what the window shows now
        restrict(&mut w, "@shownames=n");
        let hidden = crate::world::rlva::privacy::anonymous(JANE).display_name.clone();
        let told = look(&mut export, &w, now);
        let nearby: Vec<_> = told.iter().filter(|b| b.conversation == Conversation::Nearby).collect();
        assert!(nearby[0].restart, "told again, to replace");
        assert_eq!(nearby[0].lines.len(), 1);
        let line = &nearby[0].lines[0];
        assert_eq!(line.from, hidden, "the sender is the name the window shows");
        // (the text is filtered as the window filters it, no more, no less)
        assert_eq!(line.text, w.rlva_filter_text("it is me, Jane Doe"));
        let private = told.iter().find(|b| b.conversation == jane()).expect("batch");
        assert!(private.restart);
        assert_eq!(private.title, hidden);
        assert_eq!(private.lines[0].from, hidden);
        // and new lines come hidden
        say(&mut w, JANE, "Jane Doe", "again");
        let told = look(&mut export, &w, now);
        assert_eq!(texts(&told, Conversation::Nearby), [format!("{hidden}: again")]);
        // the restriction is lifted: told again, with the names back
        restrict(&mut w, "@shownames=y");
        let told = look(&mut export, &w, now);
        assert!(told.iter().all(|b| b.restart));
        // (the line heard under the restriction keeps the name Aurora
        // stored it with, in the window and here alike)
        assert_eq!(
            texts(&told, Conversation::Nearby),
            ["Jane Doe: it is me, Jane Doe".to_owned(), format!("{hidden}: again")]
        );
    }

    #[test]
    fn a_name_learnt_after_a_line_was_told_is_hidden_at_the_next_check() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        restrict(&mut w, "@shownames=n");
        look(&mut export, &w, now);
        // the text names a resident the viewer does not know yet
        say(&mut w, ME, "Me Resident", "ask Jane Doe about it");
        let told = look(&mut export, &w, now);
        assert!(format!("{told:?}").contains("Jane Doe"), "nothing to hide yet, as in the window");
        // the name is learnt: the window hides it from now on
        w.social.names.insert(JANE, "Jane Doe".into());
        w.refresh_rlva_privacy();
        if w.rlva_filter_text("ask Jane Doe about it").contains("Jane Doe") {
            // (the viewer hides only residents it has met: nothing to check)
            return;
        }
        assert!(look(&mut export, &w, now + RECHECK_EVERY / 2).is_empty(), "not checked yet");
        let told = look(&mut export, &w, now + RECHECK_EVERY * 2);
        assert!(told.iter().any(|b| b.restart && b.conversation == Conversation::Nearby));
        assert!(!format!("{told:?}").contains("Jane Doe"));
    }

    #[test]
    fn a_new_login_tells_everything_again_for_itself() {
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        say(&mut w, JANE, "Jane Doe", "hello");
        assert_eq!(look(&mut export, &w, now).len(), 1);
        export.out_of_world();
        let mut told = Vec::new();
        export.step(&w, 2, now, &mut |message| {
            if let ToHost::Chat(batch) = message {
                told.push(*batch);
            }
            true
        });
        assert_eq!(told.len(), 1);
        assert!(told[0].restart);
        assert_eq!(told[0].generation, 2);
    }

    #[test]
    fn answers_wait_for_room_in_the_link_and_keep_their_order() {
        let mut export = Export::default();
        for id in 1..=3 {
            export.answer(ToHost::SendResult {
                id,
                outcome: ipc::SendOutcome::Accepted,
            });
        }
        let mut taken = Vec::new();
        let mut room = 1;
        export.flush(&mut |message| {
            if room == 0 {
                return false;
            }
            room -= 1;
            taken.push(message);
            true
        });
        assert_eq!(taken.len(), 1);
        export.flush(&mut |message| {
            taken.push(message);
            true
        });
        let ids: Vec<_> = taken
            .iter()
            .map(|m| match m {
                ToHost::SendResult { id, .. } => *id,
                other => panic!("not an answer: {other:?}"),
            })
            .collect();
        assert_eq!(ids, [1, 2, 3]);
    }

    #[test]
    fn the_local_assistant_s_question_is_listed_first() {
        use crate::world::notifications::Data;
        let mut w = world();
        let question = || crate::mcp_bridge::desk::Question {
            number: 7,
            client: Some("Claude Code".into()),
            summary: "Move “Boots” to “Clothes”".into(),
            seconds_left: 50,
            waiting: 0,
        };
        let list = with_question(&w, Some(question()));
        assert_eq!(list.items.len(), 1);
        let item = &list.items[0];
        assert_eq!((item.kind, item.title.as_str()), (AttentionKind::Assistant, "Claude Code"));
        assert!(item.text.contains("Boots"));
        assert_eq!(item.id, ASSISTANT_ID | 7);
        // with the viewer's own questions, it keeps its place and the bound
        for n in 0..ipc::MAX_ATTENTION {
            w.notifications.push(
                Kind::Friendship,
                &format!("Friend {n}"),
                "offers friendship",
                Data::Friend {
                    from: Uuid::from_u128(n as u128 + 1),
                    tx: Uuid::nil(),
                    offline: false,
                },
            );
        }
        let list = with_question(&w, Some(question()));
        assert_eq!(list.items.len(), ipc::MAX_ATTENTION);
        assert_eq!(list.items[0].kind, AttentionKind::Assistant);
        assert_eq!(list.more, 1);
        // nothing asked: as before
        let list = with_question(&w, None);
        assert!(list.items.iter().all(|i| i.kind != AttentionKind::Assistant));
        assert_eq!(list.more, 0);
    }

    #[test]
    fn what_waits_for_an_answer_is_listed_and_the_rest_is_not() {
        use crate::world::notifications::Data;
        let (mut w, mut export, now) = (world(), Export::default(), Instant::now());
        w.notifications.push(Kind::Info, "A notice", "nothing to answer", Data::None);
        w.notifications.push(
            Kind::Friendship,
            "Jane Doe",
            "offers friendship",
            Data::Friend {
                from: JANE,
                tx: Uuid::nil(),
                offline: false,
            },
        );
        let mut lists = Vec::new();
        export.step(&w, 1, now, &mut |message| {
            if let ToHost::Attention(list) = message {
                lists.push(*list);
            }
            true
        });
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].generation, 1);
        let kinds: Vec<_> = lists[0].items.iter().map(|i| (i.kind, i.title.as_str())).collect();
        assert_eq!(kinds, [(AttentionKind::Friendship, "Jane Doe")]);
        // unchanged: not told again; answered: the list empties
        export.step(&w, 1, now, &mut |message| {
            assert!(!matches!(message, ToHost::Attention(_)), "told twice");
            true
        });
        w.notifications.list.clear();
        let mut emptied = false;
        export.step(&w, 1, now, &mut |message| {
            emptied |= matches!(&message, ToHost::Attention(list) if list.items.is_empty());
            true
        });
        assert!(emptied);
    }
}
