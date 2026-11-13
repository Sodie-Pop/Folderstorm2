//! The host's shared chat (session-host plan, section 9): what every
//! character's viewer tells of its conversations, kept per character, and
//! what the user types in the host for one of them. Pure: it is told what
//! came from a viewer, what the user did and the time, and answers with
//! [`Effect`]s for the host to carry out.
//!
//! The rules it keeps:
//! - **A line is said by the character it was typed for, or not at all.**
//!   A draft belongs to one character, one login of it and one
//!   conversation. Looking at another character or conversation shows
//!   another draft, never the same text under another name. After a
//!   logout, a login or a restart of that viewer the draft is *held*: it
//!   is not sent until the user takes it up again for the new login
//!   ([`Chat::adopt_draft`]).
//! - **Never said twice.** A line handed to a viewer's pipe is never
//!   handed again. Without the viewer's answer (its process ended, its
//!   pipe broke, or it stays silent for [`SEND_WAIT`]) the line is
//!   *uncertain*: it may have been said. The user is told, with the text,
//!   and decides. A line that never reached the pipe is *not sent*, and
//!   its text goes back to the draft.
//! - **A refusal stays in its conversation.** The viewer's reason is noted
//!   where the line was typed and the text goes back to that draft; it is
//!   never tried as another character.
//! - **Bounded.** [`MAX_LINES`] lines and [`MAX_NOTES`] notices per
//!   conversation, [`MAX_THREADS`] conversations per character,
//!   [`MAX_FEED`] entries in "All". What was dropped is marked, and is
//!   still in the character's own window: the host keeps no archive.
//! - **Nothing is answered here.** Offers, requests and script dialogs are
//!   listed for each character ([`Chat::attention`]); the answer is given
//!   in the viewer's own window.
//! - **Read in either window is read in both.** A conversation read in
//!   the host is marked read in its viewer ([`ToViewer::MarkRead`]); one
//!   read in the viewer (its unread count falls to zero there) is read in
//!   the host too (follow-ups to checkpoint 7). Only that fall counts: a
//!   viewer that shows a conversation as its lines come, behind the
//!   others, has nothing unread there, and the host still counts them for
//!   the user who has not seen them.
//! - **Closed and pinned are the host's own** (checkpoint 9). A
//!   conversation closed here leaves the host's chips and nothing else:
//!   its viewer is told nothing and keeps it open, its lines stay in
//!   "All", and a new line from someone else, or a notice of the host
//!   about a line of the user's, brings it back. A pin puts a
//!   conversation before the others; it lasts for that login.
//! - **Several windows show the chat** (checkpoint 9: the shared chat and
//!   each monitor's): what any of them shows while it has the keyboard is
//!   read ([`Chat::look_from`]).

use crate::ipc::{
    AttentionItem, AttentionList, ChatBatch, ChatLine, Conversation, LineKind, MAX_SEND, MAX_SLOTS, SendOutcome, SendRefusal, SessionKind,
    ToViewer, clean_text,
};
use crate::settings::ChatShown;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Lines kept per conversation.
pub const MAX_LINES: usize = 500;
/// The host's own notices (a refusal, an uncertain line) kept per
/// conversation, apart from its lines.
pub const MAX_NOTES: usize = 50;
/// Conversations kept per character.
pub const MAX_THREADS: usize = 64;
/// Entries of the "All" tab.
pub const MAX_FEED: usize = 1000;
/// A line handed to a viewer and not answered after this long is
/// uncertain.
pub const SEND_WAIT: Duration = Duration::from_secs(15);
/// Lines of one character that may wait for their answer at once.
pub const MAX_PENDING: usize = 8;

/// One login of one launch of a slot's viewer.
type Login = (u32, u32);

/// What the host itself says in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// The viewer did not say the line; its text is back in the draft.
    Refused { reason: SendRefusal, text: String },
    /// The line was handed to the viewer and nothing says what became of
    /// it: it may have been said. Never sent again by the host.
    Uncertain { text: String },
    /// The line never reached the viewer; its text is back in the draft.
    NotSent { text: String },
}

/// One entry of a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Line(ChatLine),
    /// That many lines were not told (the viewer dropped them first).
    Gap(u64),
    Note(Note),
}

/// One conversation of one character, as the host holds it.
#[derive(Clone, Debug)]
pub struct Thread {
    pub conversation: Conversation,
    pub kind: SessionKind,
    pub title: String,
    entries: VecDeque<Entry>,
    lines: usize,
    notes: usize,
    /// Older lines were dropped here; they are in the character's window.
    pub trimmed: bool,
    /// Lines of others not seen in the host yet.
    pub unread: u32,
    /// Nothing came for it since the character's last logout, login or
    /// restart: shown, but not written to.
    pub earlier_login: bool,
    viewer_unread: u32,
    read_asked: bool,
    last_number: Option<u64>,
    order: u64,
    /// Checkpoint 9: closed in the host (its chips do not show it).
    closed: bool,
    /// Checkpoint 9: pinned in the host, before the other conversations.
    pinned: bool,
}

impl Thread {
    fn new(conversation: Conversation, kind: SessionKind) -> Self {
        Self {
            conversation,
            kind,
            title: String::new(),
            entries: VecDeque::new(),
            lines: 0,
            notes: 0,
            trimmed: false,
            unread: 0,
            earlier_login: false,
            viewer_unread: 0,
            read_asked: false,
            last_number: None,
            order: 0,
            closed: false,
            pinned: false,
        }
    }

    /// Closed in the host: not among the chips until something new comes.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Pinned in the host.
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    /// Grows with every batch the conversation gets: the larger, the
    /// more recent.
    pub fn recency(&self) -> u64 {
        self.order
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> + ExactSizeIterator {
        self.entries.iter()
    }

    /// Forgets the lines; the host's own notices stay (an uncertain line
    /// is still uncertain).
    fn clear(&mut self) {
        self.entries.retain(|e| matches!(e, Entry::Note(_)));
        self.lines = 0;
        self.trimmed = false;
        self.last_number = None;
        self.unread = 0;
    }

    fn push(&mut self, entry: Entry) {
        match &entry {
            Entry::Line(line) => {
                self.lines += 1;
                // someone else's line brings a closed conversation back
                self.closed &= line.kind == LineKind::Own;
            }
            Entry::Note(_) => {
                self.notes += 1;
                // and so does what the host has to say of a line of the
                // user's (refused, uncertain, not sent)
                self.closed = false;
            }
            Entry::Gap(_) => {}
        }
        self.entries.push_back(entry);
        while self.lines > MAX_LINES {
            match self.entries.pop_front() {
                Some(Entry::Line(_)) => self.lines -= 1,
                Some(Entry::Note(_)) => self.notes -= 1,
                Some(Entry::Gap(_)) => {}
                None => break,
            }
            self.trimmed = true;
        }
        while self.notes > MAX_NOTES {
            let Some(oldest) = self.entries.iter().position(|e| matches!(e, Entry::Note(_))) else {
                break;
            };
            self.entries.remove(oldest);
            self.notes -= 1;
        }
    }
}

/// One entry of the "All" tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedItem {
    pub slot: usize,
    pub conversation: Conversation,
    pub entry: Entry,
}

/// What the user typed for one character in one conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    text: String,
    login: Login,
}

/// A line handed to a viewer, waiting for its answer.
#[derive(Clone, Debug)]
struct Pending {
    id: u64,
    slot: usize,
    launch: u32,
    generation: u32,
    conversation: Conversation,
    text: String,
    at: Instant,
}

#[derive(Clone, Debug, Default)]
struct Character {
    launch: u32,
    /// Its login, once its viewer told one.
    generation: Option<u32>,
    in_world: bool,
    threads: Vec<Thread>,
    attention: Vec<AttentionItem>,
    attention_more: u32,
}

impl Character {
    fn login(&self) -> Option<Login> {
        self.generation.map(|g| (self.launch, g))
    }
}

/// What the host's window shows of the chat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Viewing {
    #[default]
    Nothing,
    /// The "All" tab.
    All,
    /// One conversation of one character.
    Thread(usize, Conversation),
}

/// Why a line cannot be sent now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// The character is not in the world.
    NotInWorld,
    /// The conversation is from an earlier login, or unknown.
    EarlierLogin,
    /// The draft was typed for an earlier login: taken up again first.
    Held,
    Empty,
    /// Too many lines of this character wait for their answer.
    Busy,
}

/// What the host has to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Write to the standard input of that launch of that slot's viewer,
    /// and of no other. A `SendChat` that cannot be queued is told back
    /// with [`Chat::not_written`].
    Send { slot: usize, launch: u32, message: ToViewer },
    /// A message came for a character that is not the active one: the
    /// host may sound or flash, by what the character's alerts say of a
    /// line of someone else (`said`) or an offer (`card`) in a
    /// conversation of that kind (checkpoint 10).
    Alert {
        slot: usize,
        kind: SessionKind,
        said: bool,
        card: bool,
    },
}

/// The shared chat.
#[derive(Debug)]
pub struct Chat {
    characters: [Character; MAX_SLOTS],
    feed: VecDeque<FeedItem>,
    drafts: BTreeMap<(usize, Conversation), Draft>,
    pending: Vec<Pending>,
    next_id: u64,
    order: u64,
    /// What each of the host's windows that shows the chat has on
    /// screen, and whether it has the keyboard.
    looks: BTreeMap<u8, (Viewing, bool)>,
    active: Option<usize>,
    effects: Vec<Effect>,
    /// Checkpoint 10: what the chats show, for every character.
    shown: ChatShown,
    /// Checkpoint 10: what a window that has the keyboard shows is read.
    mark_displayed: bool,
}

/// The shared chat's own window, for [`Chat::look_from`].
pub const SHARED_WINDOW: u8 = 0;

impl Default for Chat {
    fn default() -> Self {
        Self {
            characters: Default::default(),
            feed: VecDeque::new(),
            drafts: BTreeMap::new(),
            pending: Vec::new(),
            next_id: 1,
            order: 0,
            looks: BTreeMap::new(),
            active: None,
            effects: Vec::new(),
            shown: ChatShown::default(),
            mark_displayed: true,
        }
    }
}

impl Chat {
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// The desk's active character (a message for another one alerts).
    pub fn set_active(&mut self, active: Option<usize>) {
        self.active = active;
    }

    /// Checkpoint 10: what the chats show (the host's settings). A kind
    /// that is not shown is in no chip, not in "All", counts nowhere and
    /// is never read by being shown.
    pub fn set_shown(&mut self, shown: ChatShown) {
        self.shown = shown;
    }

    pub fn shown(&self) -> ChatShown {
        self.shown
    }

    /// Checkpoint 10: what a window that has the keyboard shows is read
    /// (on, checkpoint 6's rule), or nothing is read by being shown.
    pub fn set_mark_displayed(&mut self, on: bool) {
        self.mark_displayed = on;
    }

    /// Checkpoint 10, "Auto-read the active character": every
    /// conversation of the character that is shown is read in the host.
    /// Its viewer is told nothing: what it has unread there is its own.
    pub fn read_in_host(&mut self, slot: usize) {
        let shown = self.shown;
        if let Some(c) = self.characters.get_mut(slot) {
            for t in c.threads.iter_mut().filter(|t| shown.thread(t.kind)) {
                t.unread = 0;
            }
        }
    }

    /// Whether what comes from `launch` of `slot` is taken: its current
    /// launch, or a newer one, which ends the one before.
    fn current(&mut self, slot: usize, launch: u32) -> bool {
        let Some(c) = self.characters.get(slot) else {
            return false;
        };
        if launch < c.launch {
            return false;
        }
        if launch > c.launch {
            let before = c.launch;
            self.ended(slot, before);
            self.characters[slot].launch = launch;
        }
        true
    }

    /// The character's login changed: what was typed and shown for the
    /// one before no longer counts for this one.
    fn set_generation(&mut self, slot: usize, generation: Option<u32>) {
        let c = &mut self.characters[slot];
        if c.generation == generation {
            return;
        }
        c.generation = generation;
        for t in &mut c.threads {
            t.earlier_login = true;
            t.viewer_unread = 0;
            t.read_asked = false;
            // (a pin is for one login: the next one's conversations are
            // other ones)
            t.pinned = false;
        }
        c.attention.clear();
        c.attention_more = 0;
    }

    /// A viewer's report: its login, and whether it is in the world.
    pub fn status(&mut self, slot: usize, launch: u32, generation: u32, in_world: bool) {
        if !self.current(slot, launch) {
            return;
        }
        self.set_generation(slot, Some(generation));
        self.characters[slot].in_world = in_world;
    }

    /// That launch of the slot's viewer ended, or its link did: the lines
    /// it had not answered are uncertain.
    pub fn ended(&mut self, slot: usize, launch: u32) {
        if self.characters.get(slot).is_none_or(|c| c.launch != launch) {
            return;
        }
        let (lost, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| p.slot == slot && p.launch == launch);
        self.pending = kept;
        for p in lost {
            self.note(slot, p.conversation, Note::Uncertain { text: p.text });
        }
        self.set_generation(slot, None);
        self.characters[slot].in_world = false;
    }

    /// The slot holds no character any more (removed from the desk):
    /// nothing of it stays, so nothing of it can meet the next one.
    pub fn forget(&mut self, slot: usize) {
        let Some(c) = self.characters.get_mut(slot) else {
            return;
        };
        // (the launch number stays: an old viewer's late words are dropped)
        *c = Character {
            launch: c.launch,
            ..Character::default()
        };
        self.drafts.retain(|(s, _), _| *s != slot);
        self.pending.retain(|p| p.slot != slot);
        self.feed.retain(|f| f.slot != slot);
    }

    fn thread_mut(&mut self, slot: usize, conversation: Conversation) -> Option<&mut Thread> {
        self.characters
            .get_mut(slot)?
            .threads
            .iter_mut()
            .find(|t| t.conversation == conversation)
    }

    fn push(&mut self, slot: usize, conversation: Conversation, entry: Entry) {
        if let Some(t) = self.thread_mut(slot, conversation) {
            t.push(entry.clone());
        }
        self.feed.push_back(FeedItem { slot, conversation, entry });
        while self.feed.len() > MAX_FEED {
            self.feed.pop_front();
        }
    }

    fn note(&mut self, slot: usize, conversation: Conversation, note: Note) {
        self.push(slot, conversation, Entry::Note(note));
    }

    /// On screen in a window that has the keyboard.
    fn viewed(&self, slot: usize, conversation: Conversation) -> bool {
        // (checkpoint 10: unless nothing is read by being shown, and never
        // a kind the chats do not show)
        if !self.mark_displayed || self.thread(slot, conversation).is_some_and(|t| !self.shown.thread(t.kind)) {
            return false;
        }
        self.looks.values().any(|(viewing, focused)| {
            *focused
                && match *viewing {
                    Viewing::Nothing => false,
                    Viewing::All => true,
                    Viewing::Thread(s, c) => s == slot && c == conversation,
                }
        })
    }

    /// Chosen in one of the host's windows, with the keyboard or not.
    fn on_screen(&self, slot: usize, conversation: Conversation) -> bool {
        self.looks
            .values()
            .any(|(viewing, _)| *viewing == Viewing::Thread(slot, conversation))
    }

    /// New lines of a conversation, from that launch of the slot's viewer.
    pub fn batch(&mut self, slot: usize, launch: u32, batch: ChatBatch) {
        let batch = batch.cleaned();
        // (a conversation and its kind agree, or the batch is not one)
        if (batch.conversation == Conversation::Nearby) != (batch.kind == SessionKind::Nearby) || !self.current(slot, launch) {
            return;
        }
        self.set_generation(slot, Some(batch.generation));
        let conversation = batch.conversation;
        self.order += 1;
        let order = self.order;
        let on_screen: Vec<Conversation> = self.characters[slot]
            .threads
            .iter()
            .map(|t| t.conversation)
            .filter(|c| self.on_screen(slot, *c))
            .collect();
        let c = &mut self.characters[slot];
        if !c.threads.iter().any(|t| t.conversation == conversation) {
            let thread = Thread::new(conversation, batch.kind);
            if conversation == Conversation::Nearby {
                c.threads.insert(0, thread);
            } else {
                c.threads.push(thread);
            }
            if c.threads.len() > MAX_THREADS {
                // the conversation heard of longest ago leaves, never the
                // nearby chat, the new one or the one on screen
                let oldest = c
                    .threads
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        t.conversation != Conversation::Nearby && t.conversation != conversation && !on_screen.contains(&t.conversation)
                    })
                    .min_by_key(|(_, t)| t.order)
                    .map(|(i, _)| i);
                if let Some(oldest) = oldest {
                    let gone = c.threads.remove(oldest).conversation;
                    self.drafts.remove(&(slot, gone));
                }
            }
        }
        let viewed = self.viewed(slot, conversation);
        let Some(t) = self.thread_mut(slot, conversation) else {
            return;
        };
        t.order = order;
        t.kind = batch.kind;
        t.title = batch.title;
        // the viewer had unread lines and has none now: read there
        let read_there = t.viewer_unread > 0 && batch.unread == 0 && !batch.restart;
        t.viewer_unread = batch.unread;
        if batch.unread == 0 {
            t.read_asked = false;
        }
        // (the numbers of an earlier login name other lines)
        if batch.restart || std::mem::take(&mut t.earlier_login) {
            t.clear();
            self.feed
                .retain(|f| !(f.slot == slot && f.conversation == conversation && !matches!(f.entry, Entry::Note(_))));
        }
        if batch.missed > 0 {
            self.push(slot, conversation, Entry::Gap(u64::from(batch.missed)));
        }
        let (mut unread, mut said, mut card) = (0u32, false, false);
        for line in batch.lines {
            let Some(t) = self.thread_mut(slot, conversation) else {
                break;
            };
            if t.last_number.is_some_and(|last| line.number <= last) {
                continue;
            }
            t.last_number = Some(line.number);
            let others = matches!(line.kind, LineKind::Said | LineKind::Object | LineKind::Card);
            unread += u32::from(others);
            if conversation != Conversation::Nearby {
                said |= line.kind == LineKind::Said;
                card |= line.kind == LineKind::Card;
            }
            self.push(slot, conversation, Entry::Line(line));
        }
        if let Some(t) = self.thread_mut(slot, conversation) {
            if read_there {
                t.unread = 0;
            } else if !viewed {
                t.unread = t.unread.saturating_add(unread);
            }
        }
        let kind = self.thread(slot, conversation).map_or(SessionKind::Nearby, |t| t.kind);
        if (said || card) && self.active != Some(slot) && !batch.restart && self.shown.thread(kind) {
            self.effects.push(Effect::Alert { slot, kind, said, card });
        }
        if viewed {
            self.read(slot, conversation);
        }
    }

    /// The conversation is on screen: nothing of it is unread, and its
    /// viewer is told once.
    fn read(&mut self, slot: usize, conversation: Conversation) {
        let Some(login) = self.characters.get(slot).and_then(Character::login) else {
            return;
        };
        let Some(t) = self.thread_mut(slot, conversation) else {
            return;
        };
        t.unread = 0;
        if t.viewer_unread == 0 || t.read_asked || t.earlier_login || conversation == Conversation::Nearby {
            return;
        }
        t.read_asked = true;
        self.effects.push(Effect::Send {
            slot,
            launch: login.0,
            message: ToViewer::MarkRead {
                generation: login.1,
                conversation,
            },
        });
    }

    /// What the shared chat's window shows now, and whether it has the
    /// keyboard: what is on screen in a window the user is at is read.
    pub fn look(&mut self, viewing: Viewing, focused: bool) {
        self.look_from(SHARED_WINDOW, viewing, focused);
    }

    /// The same for any window of the host that shows the chat (`window`:
    /// [`SHARED_WINDOW`], or a number of the host's choosing per
    /// monitor). A window that shows nothing, or is gone, says
    /// [`Viewing::Nothing`].
    pub fn look_from(&mut self, window: u8, viewing: Viewing, focused: bool) {
        if viewing == Viewing::Nothing {
            self.looks.remove(&window);
        } else {
            self.looks.insert(window, (viewing, focused));
        }
        if !focused || !self.mark_displayed {
            return;
        }
        let shown = self.shown;
        let seen: Vec<(usize, Conversation)> = match viewing {
            Viewing::Nothing => Vec::new(),
            Viewing::Thread(slot, conversation) => self
                .thread(slot, conversation)
                .filter(|t| shown.thread(t.kind))
                .map(|t| (slot, t.conversation))
                .into_iter()
                .collect(),
            Viewing::All => self
                .characters
                .iter()
                .enumerate()
                .flat_map(|(slot, c)| {
                    c.threads
                        .iter()
                        .filter(move |t| shown.thread(t.kind))
                        .map(move |t| (slot, t.conversation))
                })
                .collect(),
        };
        for (slot, conversation) in seen {
            self.read(slot, conversation);
        }
    }

    /// What waits for an answer in that launch of the slot's viewer.
    pub fn attention_list(&mut self, slot: usize, launch: u32, list: AttentionList) {
        let list = list.cleaned();
        if !self.current(slot, launch) || self.characters[slot].generation != Some(list.generation) {
            return;
        }
        let c = &mut self.characters[slot];
        c.attention = list.items;
        c.attention_more = list.more;
    }

    /// The viewer's answer to line `id`. Only the first answer of the
    /// launch the line was handed to counts.
    pub fn result(&mut self, slot: usize, launch: u32, id: u64, outcome: SendOutcome) {
        let Some(at) = self.pending.iter().position(|p| p.id == id && p.slot == slot && p.launch == launch) else {
            return;
        };
        let p = self.pending.remove(at);
        if let SendOutcome::Refused(reason) = outcome {
            self.give_back(&p);
            self.note(slot, p.conversation, Note::Refused { reason, text: p.text });
        }
    }

    /// Line `id` could not be handed to its viewer's pipe at all: it was
    /// not sent, for certain.
    pub fn not_written(&mut self, id: u64) {
        let Some(at) = self.pending.iter().position(|p| p.id == id) else {
            return;
        };
        let p = self.pending.remove(at);
        self.give_back(&p);
        self.note(p.slot, p.conversation, Note::NotSent { text: p.text });
    }

    /// The text of a line that was not said goes back to its own draft,
    /// with the login it was typed for, unless the user typed again there.
    fn give_back(&mut self, p: &Pending) {
        self.drafts.entry((p.slot, p.conversation)).or_insert_with(|| Draft {
            text: p.text.clone(),
            login: (p.launch, p.generation),
        });
    }

    /// Now and then: lines without an answer for [`SEND_WAIT`] are
    /// uncertain. They are never handed again.
    pub fn tick(&mut self, now: Instant) {
        let (late, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| now.saturating_duration_since(p.at) >= SEND_WAIT);
        self.pending = kept;
        for p in late {
            self.note(p.slot, p.conversation, Note::Uncertain { text: p.text });
        }
    }

    /// The character's viewer told its login: lines can be typed for it.
    pub fn logged_in(&self, slot: usize) -> bool {
        self.characters.get(slot).is_some_and(|c| c.generation.is_some())
    }

    /// What is typed for that character in that conversation.
    pub fn draft(&self, slot: usize, conversation: Conversation) -> &str {
        self.drafts.get(&(slot, conversation)).map_or("", |d| d.text.as_str())
    }

    /// The draft was typed for an earlier login of the character (or the
    /// character is logged out): shown, not sent.
    pub fn draft_held(&self, slot: usize, conversation: Conversation) -> bool {
        let login = self.characters.get(slot).and_then(Character::login);
        self.drafts.get(&(slot, conversation)).is_some_and(|d| Some(d.login) != login)
    }

    /// The user types for that character in that conversation. False, and
    /// nothing changes, while the character has no login or the draft is
    /// held.
    pub fn set_draft(&mut self, slot: usize, conversation: Conversation, text: &str) -> bool {
        let Some(login) = self.characters.get(slot).and_then(Character::login) else {
            return false;
        };
        if self.draft_held(slot, conversation) {
            return false;
        }
        if text.is_empty() {
            self.drafts.remove(&(slot, conversation));
        } else {
            let text: String = text.chars().take(MAX_SEND).collect();
            self.drafts.insert((slot, conversation), Draft { text, login });
        }
        true
    }

    /// The user takes a held draft up again, for the character as it is
    /// logged in now.
    pub fn adopt_draft(&mut self, slot: usize, conversation: Conversation) -> bool {
        let Some(login) = self.characters.get(slot).and_then(Character::login) else {
            return false;
        };
        match self.drafts.get_mut(&(slot, conversation)) {
            Some(draft) => {
                draft.login = login;
                true
            }
            None => false,
        }
    }

    pub fn discard_draft(&mut self, slot: usize, conversation: Conversation) {
        self.drafts.remove(&(slot, conversation));
    }

    /// Whether the draft of that conversation could be sent now.
    pub fn can_send(&self, slot: usize, conversation: Conversation) -> Result<(), Blocked> {
        let c = self.characters.get(slot).ok_or(Blocked::NotInWorld)?;
        if !c.in_world || c.generation.is_none() {
            return Err(Blocked::NotInWorld);
        }
        if c.threads
            .iter()
            .find(|t| t.conversation == conversation)
            .is_none_or(|t| t.earlier_login)
        {
            return Err(Blocked::EarlierLogin);
        }
        if self.draft_held(slot, conversation) {
            return Err(Blocked::Held);
        }
        if clean_text(self.draft(slot, conversation), MAX_SEND).is_empty() {
            return Err(Blocked::Empty);
        }
        if self.pending.iter().filter(|p| p.slot == slot).count() >= MAX_PENDING {
            return Err(Blocked::Busy);
        }
        Ok(())
    }

    /// Sends the draft of that conversation as that character: handed to
    /// its viewer once, with the login it was typed for.
    pub fn submit(&mut self, slot: usize, conversation: Conversation, now: Instant) -> Result<u64, Blocked> {
        self.can_send(slot, conversation)?;
        let (Some(draft), Some((launch, generation))) = (self.drafts.remove(&(slot, conversation)), self.characters[slot].login()) else {
            return Err(Blocked::Empty);
        };
        let text = clean_text(&draft.text, MAX_SEND);
        let id = self.next_id;
        self.next_id += 1;
        self.pending.push(Pending {
            id,
            slot,
            launch,
            generation,
            conversation,
            text: text.clone(),
            at: now,
        });
        self.effects.push(Effect::Send {
            slot,
            launch,
            message: ToViewer::SendChat {
                id,
                generation,
                conversation,
                text,
            },
        });
        Ok(id)
    }

    /// Checkpoint 9: closes a conversation in the host. Its chip goes,
    /// with what was typed for it and its pin; nothing is said to its
    /// viewer, where it stays as it is, and its lines stay in "All". A
    /// new line from someone else brings it back. Never the nearby chat.
    /// True when it was open.
    pub fn close(&mut self, slot: usize, conversation: Conversation) -> bool {
        if conversation == Conversation::Nearby {
            return false;
        }
        let Some(t) = self.thread_mut(slot, conversation).filter(|t| !t.closed) else {
            return false;
        };
        t.closed = true;
        t.pinned = false;
        t.unread = 0;
        self.drafts.remove(&(slot, conversation));
        true
    }

    /// Checkpoint 9: pins a conversation before the others in the host's
    /// chips, or lets it go. Host only; forgotten with the character's
    /// login. True when that changed something.
    pub fn set_pinned(&mut self, slot: usize, conversation: Conversation, pinned: bool) -> bool {
        match self.thread_mut(slot, conversation).filter(|t| t.pinned != pinned && !t.closed) {
            Some(t) => {
                t.pinned = pinned;
                true
            }
            None => false,
        }
    }

    /// The character's conversations that are not closed in the host,
    /// the nearby chat first: what its chips show.
    /// (Checkpoint 10: and of a kind the chats show.)
    pub fn open_threads(&self, slot: usize) -> impl Iterator<Item = &Thread> {
        let shown = self.shown;
        self.threads(slot).iter().filter(move |t| !t.closed && shown.thread(t.kind))
    }

    /// One of them; None for a conversation that is closed, not shown or
    /// unknown.
    pub fn open_thread(&self, slot: usize, conversation: Conversation) -> Option<&Thread> {
        self.thread(slot, conversation).filter(|t| !t.closed && self.shown.thread(t.kind))
    }

    /// How many lines typed in that conversation wait for their answer.
    pub fn sending(&self, slot: usize, conversation: Conversation) -> usize {
        self.pending
            .iter()
            .filter(|p| p.slot == slot && p.conversation == conversation)
            .count()
    }

    /// The character's conversations, the nearby chat first, those
    /// closed in the host among them.
    pub fn threads(&self, slot: usize) -> &[Thread] {
        self.characters.get(slot).map_or(&[], |c| c.threads.as_slice())
    }

    pub fn thread(&self, slot: usize, conversation: Conversation) -> Option<&Thread> {
        self.threads(slot).iter().find(|t| t.conversation == conversation)
    }

    /// The "All" tab: every character's entries as they came (checkpoint
    /// 10: those the chats show).
    pub fn feed(&self) -> impl DoubleEndedIterator<Item = &FeedItem> {
        self.feed.iter().filter(|item| self.feed_shown(item))
    }

    /// An entry of a conversation that is shown is shown too, but a line of
    /// the viewer's when its notices are not (checkpoint 10).
    pub fn shows_entry(&self, entry: &Entry) -> bool {
        match entry {
            Entry::Line(line) => self.shown.line(line.kind),
            Entry::Gap(_) | Entry::Note(_) => true,
        }
    }

    /// An entry of "All" is shown: its conversation's kind is, and a line
    /// of the viewer's is when its notices are.
    fn feed_shown(&self, item: &FeedItem) -> bool {
        let kind = self.thread(item.slot, item.conversation).map_or(SessionKind::Nearby, |t| t.kind);
        self.shown.thread(kind) && self.shows_entry(&item.entry)
    }

    /// The character's unread lines, all the conversations shown together.
    pub fn unread(&self, slot: usize) -> u32 {
        self.threads(slot)
            .iter()
            .filter(|t| self.shown.thread(t.kind))
            .fold(0, |sum, t| sum.saturating_add(t.unread))
    }

    pub fn total_unread(&self) -> u32 {
        (0..MAX_SLOTS).fold(0, |sum, slot| sum.saturating_add(self.unread(slot)))
    }

    /// The login the character's conversations and questions are of: its
    /// viewer's launch and that launch's login; None while it is logged
    /// out, has no viewer, or is not managed.
    pub fn login(&self, slot: usize) -> Option<(u32, u32)> {
        self.characters.get(slot)?.login()
    }

    /// What waits for an answer in the character's own window, and how
    /// many more than listed.
    pub fn attention(&self, slot: usize) -> (&[AttentionItem], u32) {
        self.characters
            .get(slot)
            .map_or((&[], 0), |c| (c.attention.as_slice(), c.attention_more))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JANE: Conversation = Conversation::Session(0, 0x1a);
    const GROUP: Conversation = Conversation::Session(7, 0x9);

    fn line(number: u64, kind: LineKind, from: &str, text: &str) -> ChatLine {
        ChatLine {
            number,
            time: 1_800_000_000 + number,
            kind,
            emote: false,
            from: from.into(),
            text: text.into(),
        }
    }

    fn said(number: u64, text: &str) -> ChatLine {
        line(number, LineKind::Said, "Jane Doe", text)
    }

    fn batch(generation: u32, conversation: Conversation, lines: Vec<ChatLine>) -> ChatBatch {
        ChatBatch {
            generation,
            conversation,
            kind: match conversation {
                Conversation::Nearby => SessionKind::Nearby,
                c if c == GROUP => SessionKind::Group,
                _ => SessionKind::Resident,
            },
            title: if conversation == JANE {
                "Jane Doe".into()
            } else {
                "Somewhere".into()
            },
            lines,
            ..Default::default()
        }
    }

    /// A chat with `count` characters in the world, launch 1, login 1.
    fn chat(count: usize) -> (Chat, Instant) {
        let mut c = Chat::default();
        for slot in 0..count {
            c.status(slot, 1, 1, true);
        }
        (c, Instant::now())
    }

    fn texts(c: &Chat, slot: usize, conversation: Conversation) -> Vec<String> {
        c.thread(slot, conversation)
            .expect("thread")
            .entries()
            .map(|e| match e {
                Entry::Line(l) => l.text.clone(),
                Entry::Gap(n) => format!("<gap {n}>"),
                Entry::Note(Note::Refused { reason, text }) => format!("<refused {reason:?}: {text}>"),
                Entry::Note(Note::Uncertain { text }) => format!("<uncertain: {text}>"),
                Entry::Note(Note::NotSent { text }) => format!("<not sent: {text}>"),
            })
            .collect()
    }

    /// The lines the host was asked to hand to viewers.
    fn sends(c: &mut Chat) -> Vec<(usize, u32, ToViewer)> {
        c.take_effects()
            .into_iter()
            .filter_map(|e| match e {
                Effect::Send { slot, launch, message } => Some((slot, launch, message)),
                Effect::Alert { .. } => None,
            })
            .collect()
    }

    #[test]
    fn each_character_has_its_conversations_and_all_has_them_together() {
        let (mut c, _) = chat(2);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi Sam")]));
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(1, "hello all")]));
        c.batch(1, 1, batch(1, JANE, vec![said(1, "hi Alex")]));
        // the nearby chat first, whatever came first
        let kinds: Vec<_> = c.threads(0).iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [SessionKind::Nearby, SessionKind::Resident]);
        assert_eq!(c.threads(1).len(), 1);
        assert_eq!(texts(&c, 0, JANE), ["hi Sam"]);
        assert_eq!(
            texts(&c, 1, JANE),
            ["hi Alex"],
            "the same resident, another character: another conversation"
        );
        let all: Vec<_> = c
            .feed()
            .map(|f| (f.slot, matches!(f.entry, Entry::Line(_)), f.conversation))
            .collect();
        assert_eq!(all, [(0, true, JANE), (0, true, Conversation::Nearby), (1, true, JANE)]);
        assert_eq!(c.thread(0, JANE).expect("thread").title, "Jane Doe");
    }

    #[test]
    fn a_line_told_twice_is_kept_once_and_a_restart_replaces() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "one"), said(2, "two")]));
        c.batch(0, 1, batch(1, JANE, vec![said(2, "two"), said(3, "three")]));
        assert_eq!(texts(&c, 0, JANE), ["one", "two", "three"]);
        // the viewer hides a name now: what the host held is replaced
        let mut again = batch(1, JANE, vec![said(1, "one"), said(2, "A resident says two")]);
        again.restart = true;
        again.title = "A resident".into();
        c.batch(0, 1, again);
        assert_eq!(texts(&c, 0, JANE), ["one", "A resident says two"]);
        assert_eq!(c.thread(0, JANE).expect("thread").title, "A resident");
        assert_eq!(c.feed().count(), 2, "the All tab forgets the replaced lines too");
        assert!(
            c.feed()
                .all(|f| !matches!(&f.entry, Entry::Line(l) if l.text == "two" || l.text == "three"))
        );
    }

    #[test]
    fn history_is_bounded_and_what_is_missing_is_marked() {
        let (mut c, _) = chat(1);
        let mut gap = batch(1, JANE, vec![said(10, "after the gap")]);
        gap.missed = 4;
        c.batch(0, 1, batch(1, JANE, vec![said(1, "first")]));
        c.batch(0, 1, gap);
        assert_eq!(texts(&c, 0, JANE), ["first", "<gap 4>", "after the gap"]);
        assert!(!c.thread(0, JANE).expect("thread").trimmed);
        for n in 0..(MAX_LINES as u64 + 40) {
            c.batch(0, 1, batch(1, JANE, vec![said(11 + n, "more")]));
        }
        let t = c.thread(0, JANE).expect("thread");
        assert_eq!(t.entries().filter(|e| matches!(e, Entry::Line(_))).count(), MAX_LINES);
        assert!(t.trimmed, "the user is told older lines are in the character's window");
        assert!(!texts(&c, 0, JANE).contains(&"first".to_owned()));
        assert!(c.feed().count() <= MAX_FEED);
        for n in 0..(MAX_FEED as u64 * 2) {
            c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(n + 1, "busy region")]));
        }
        assert_eq!(c.feed().count(), MAX_FEED);
    }

    #[test]
    fn notices_have_their_own_bound_and_never_push_lines_out() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "kept")]));
        for _ in 0..(MAX_NOTES + 20) {
            assert!(c.set_draft(0, JANE, "x"));
            let id = c.submit(0, JANE, now).expect("sent");
            c.result(0, 1, id, SendOutcome::Refused(SendRefusal::Restricted));
            c.discard_draft(0, JANE);
        }
        let t = c.thread(0, JANE).expect("thread");
        assert_eq!(t.entries().filter(|e| matches!(e, Entry::Note(_))).count(), MAX_NOTES);
        assert!(matches!(t.entries().next(), Some(Entry::Line(l)) if l.text == "kept"));
        assert!(!t.trimmed);
    }

    #[test]
    fn conversations_are_bounded_and_the_nearby_chat_stays() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(1, "near")]));
        c.batch(0, 1, batch(1, JANE, vec![said(1, "first resident")]));
        c.look(Viewing::Thread(0, JANE), false);
        for n in 0..(MAX_THREADS as u64 * 2) {
            c.batch(0, 1, batch(1, Conversation::Session(1, 100 + n), vec![said(1, "hi")]));
        }
        assert_eq!(c.threads(0).len(), MAX_THREADS);
        assert!(c.thread(0, Conversation::Nearby).is_some());
        assert!(c.thread(0, JANE).is_some(), "the conversation on screen stays");
        assert!(
            c.thread(0, Conversation::Session(1, 100)).is_none(),
            "the one heard of longest ago left"
        );
        assert!(c.thread(0, Conversation::Session(1, 100 + MAX_THREADS as u64 * 2 - 1)).is_some());
    }

    #[test]
    fn unread_counts_others_lines_until_they_are_on_screen() {
        let (mut c, _) = chat(2);
        c.batch(
            0,
            1,
            batch(
                1,
                JANE,
                vec![
                    said(1, "hi"),
                    line(2, LineKind::Own, "Sam", "hello"),
                    line(3, LineKind::System, "", "Jane is online"),
                    line(4, LineKind::Card, "Jane Doe", "offers a teleport"),
                ],
            ),
        );
        c.batch(1, 1, batch(1, GROUP, vec![said(1, "meeting")]));
        assert_eq!((c.unread(0), c.unread(1), c.total_unread()), (2, 1, 3));
        // on screen in a window that has not the keyboard: still unread
        c.look(Viewing::Thread(0, JANE), false);
        assert_eq!(c.unread(0), 2);
        c.look(Viewing::Thread(0, JANE), true);
        assert_eq!((c.unread(0), c.unread(1)), (0, 1));
        // what comes while it is on screen is read at once
        c.batch(0, 1, batch(1, JANE, vec![said(5, "still there?")]));
        assert_eq!(c.unread(0), 0);
        // the All tab reads everything
        c.look(Viewing::All, true);
        assert_eq!(c.total_unread(), 0);
    }

    #[test]
    fn the_viewer_is_told_once_that_a_conversation_was_read() {
        let (mut c, _) = chat(1);
        let mut b = batch(1, JANE, vec![said(1, "hi")]);
        b.unread = 1;
        c.batch(0, 1, b);
        let mut near = batch(1, Conversation::Nearby, vec![said(1, "hi")]);
        near.unread = 1;
        c.batch(0, 1, near);
        assert!(sends(&mut c).is_empty());
        c.look(Viewing::All, true);
        c.look(Viewing::All, true);
        let read = ToViewer::MarkRead {
            generation: 1,
            conversation: JANE,
        };
        assert_eq!(sends(&mut c), vec![(0, 1, read.clone())], "once, and never for the nearby chat");
        // the viewer says it is read, then a new line comes while on screen
        c.batch(0, 1, batch(1, JANE, vec![]));
        let mut b = batch(1, JANE, vec![said(2, "again")]);
        b.unread = 1;
        c.batch(0, 1, b);
        assert_eq!(sends(&mut c), vec![(0, 1, read)]);
    }

    #[test]
    fn a_conversation_read_in_its_viewer_is_read_in_the_host() {
        let (mut c, _) = chat(1);
        let mut b = batch(1, JANE, vec![said(1, "hi"), said(2, "there?")]);
        b.unread = 2;
        c.batch(0, 1, b);
        assert_eq!(c.unread(0), 2);
        // a line more, still unread there
        let mut b = batch(1, JANE, vec![said(3, "hello?")]);
        b.unread = 3;
        c.batch(0, 1, b);
        assert_eq!(c.unread(0), 3);
        // read in the viewer's own window: nothing new, its count falls
        c.batch(0, 1, batch(1, JANE, vec![]));
        assert_eq!(c.unread(0), 0);
        assert!(sends(&mut c).is_empty(), "nothing to tell the viewer back");
        // a viewer that has the conversation on screen as lines come
        // (behind, the user elsewhere): nothing unread there, still
        // unread in the host
        c.batch(0, 1, batch(1, JANE, vec![said(4, "are you there?")]));
        assert_eq!(c.unread(0), 1);
        // the lines and the fall in one look: read there, read here
        let mut b = batch(1, JANE, vec![said(5, "one")]);
        b.unread = 1;
        c.batch(0, 1, b);
        assert_eq!(c.unread(0), 2);
        c.batch(0, 1, batch(1, JANE, vec![said(6, "two")]));
        assert_eq!(c.unread(0), 0);
        // the nearby chat has no count in the viewer: the host's stays
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(1, "near")]));
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![]));
        assert_eq!(c.unread(0), 1);
    }

    #[test]
    fn a_background_message_alerts_and_the_active_one_or_nearby_chat_does_not() {
        let (mut c, _) = chat(2);
        c.set_active(Some(0));
        c.batch(0, 1, batch(1, JANE, vec![said(1, "to the active one")]));
        c.batch(1, 1, batch(1, Conversation::Nearby, vec![said(1, "near a background one")]));
        c.batch(1, 1, batch(1, JANE, vec![line(1, LineKind::Own, "Alex", "my own line")]));
        assert_eq!(c.take_effects(), vec![]);
        c.batch(1, 1, batch(1, JANE, vec![said(2, "to a background one"), said(3, "twice")]));
        let alert = Effect::Alert {
            slot: 1,
            kind: SessionKind::Resident,
            said: true,
            card: false,
        };
        assert_eq!(c.take_effects(), vec![alert]);
        // a conversation told again is not news
        let mut again = batch(1, JANE, vec![said(2, "to a background one")]);
        again.restart = true;
        c.batch(1, 1, again);
        assert_eq!(c.take_effects(), vec![]);
        // an offer in a group says what it is (checkpoint 10)
        c.batch(1, 1, batch(1, GROUP, vec![line(1, LineKind::Card, "Shop", "an offer")]));
        let card = Effect::Alert {
            slot: 1,
            kind: SessionKind::Group,
            said: false,
            card: true,
        };
        assert_eq!(c.take_effects(), vec![card]);
        // a kind the chats do not show alerts nobody
        c.set_shown(ChatShown {
            ims: false,
            ..ChatShown::default()
        });
        c.batch(1, 1, batch(1, JANE, vec![said(4, "hidden")]));
        assert_eq!(c.take_effects(), vec![]);
    }

    #[test]
    fn a_kind_the_chats_do_not_show_is_in_no_chip_nor_all_and_never_read_by_being_shown() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(1, "near")]));
        c.batch(0, 1, batch(1, JANE, vec![said(1, "an im")]));
        c.batch(
            0,
            1,
            batch(1, GROUP, vec![said(1, "a group"), line(2, LineKind::System, "", "a notice")]),
        );
        assert_eq!(c.open_threads(0).count(), 3);
        assert_eq!(c.feed().count(), 4);
        assert_eq!(c.unread(0), 3);
        c.set_shown(ChatShown {
            groups: false,
            notices: false,
            ..ChatShown::default()
        });
        assert_eq!(c.open_threads(0).count(), 2);
        assert!(c.open_thread(0, GROUP).is_none());
        assert_eq!(c.feed().count(), 2, "no group line, no notice");
        assert_eq!(c.unread(0), 2, "the group's unread line counts nowhere");
        // "All" on screen in a window that has the keyboard reads what it
        // shows, and not the group
        c.look(Viewing::All, true);
        assert_eq!(c.unread(0), 0);
        assert_eq!(c.thread(0, GROUP).map(|t| t.unread), Some(1));
        c.look(Viewing::Thread(0, GROUP), true);
        assert_eq!(c.thread(0, GROUP).map(|t| t.unread), Some(1), "a hidden kind is never read");
        // the notice of the viewer is hidden in a conversation that shows
        let notice = Entry::Line(line(9, LineKind::System, "", "x"));
        assert!(!c.shows_entry(&notice));
        c.set_shown(ChatShown::default());
        assert!(c.shows_entry(&notice));
        assert_eq!(c.unread(0), 1);
    }

    #[test]
    fn with_mark_displayed_read_off_nothing_is_read_by_being_shown_and_auto_read_is_the_host_s() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "one"), said(2, "two")]));
        assert_eq!(c.unread(0), 2);
        c.set_mark_displayed(false);
        c.look(Viewing::Thread(0, JANE), true);
        assert_eq!(c.unread(0), 2);
        assert!(sends(&mut c).is_empty(), "nothing told to the viewer");
        // auto-read: read in the host, the viewer is told nothing
        c.read_in_host(0);
        assert_eq!(c.unread(0), 0);
        assert!(sends(&mut c).is_empty());
        c.set_mark_displayed(true);
        c.batch(0, 1, batch(1, JANE, vec![said(3, "three")]));
        c.look(Viewing::Thread(0, JANE), true);
        assert_eq!(c.unread(0), 0);
    }

    #[test]
    fn a_draft_stays_with_its_character_and_conversation() {
        let (mut c, now) = chat(2);
        for slot in 0..2 {
            c.batch(slot, 1, batch(1, JANE, vec![said(1, "hi")]));
            c.batch(slot, 1, batch(1, Conversation::Nearby, vec![said(1, "hi")]));
        }
        assert!(c.set_draft(0, JANE, "typed as Sam to Jane"));
        // looking at another character or conversation shows another draft
        assert_eq!(c.draft(1, JANE), "");
        assert_eq!(c.draft(0, Conversation::Nearby), "");
        assert_eq!(c.can_send(1, JANE), Err(Blocked::Empty));
        assert_eq!(c.submit(1, JANE, now), Err(Blocked::Empty));
        assert_eq!(c.submit(0, Conversation::Nearby, now), Err(Blocked::Empty));
        assert!(sends(&mut c).is_empty(), "nothing was sent as another character or elsewhere");
        assert!(c.set_draft(1, JANE, "typed as Alex"));
        assert_eq!(c.draft(0, JANE), "typed as Sam to Jane");
        // sent: to slot 0's viewer only, in Jane's conversation, as typed
        let id = c.submit(0, JANE, now).expect("sent");
        assert_eq!(
            sends(&mut c),
            vec![(
                0,
                1,
                ToViewer::SendChat {
                    id,
                    generation: 1,
                    conversation: JANE,
                    text: "typed as Sam to Jane".into()
                }
            )]
        );
        assert_eq!(c.draft(0, JANE), "");
        assert_eq!(c.draft(1, JANE), "typed as Alex");
        assert_eq!(c.sending(0, JANE), 1);
        c.result(0, 1, id, SendOutcome::Accepted);
        assert_eq!(c.sending(0, JANE), 0);
        assert_eq!(
            texts(&c, 0, JANE),
            ["hi"],
            "what was said comes back from the viewer, not from the host"
        );
    }

    #[test]
    fn a_relogin_holds_the_draft_until_the_user_takes_it_up_again() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "typed before the logout"));
        // logged out: the login changes
        c.status(0, 1, 2, false);
        assert!(c.draft_held(0, JANE));
        assert_eq!(c.can_send(0, JANE), Err(Blocked::NotInWorld));
        // logged in again, perhaps as another account
        c.status(0, 1, 3, true);
        assert!(c.thread(0, JANE).expect("thread").earlier_login);
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::EarlierLogin));
        c.batch(0, 1, batch(3, JANE, vec![said(1, "back?")]));
        assert_eq!(c.draft(0, JANE), "typed before the logout", "the text is not lost");
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::Held));
        assert!(!c.set_draft(0, JANE, "typed over it"), "a held draft is not typed into");
        assert!(sends(&mut c).is_empty());
        assert!(c.adopt_draft(0, JANE));
        assert!(!c.draft_held(0, JANE));
        let id = c.submit(0, JANE, now).expect("sent");
        assert!(matches!(
            sends(&mut c).as_slice(),
            [(0, 1, ToViewer::SendChat { id: sent, generation: 3, .. })] if *sent == id
        ));
    }

    #[test]
    fn a_restarted_viewer_holds_the_draft_even_with_the_same_login_number() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "typed for the first viewer"));
        // the viewer closes and is started again: launch 2, whose first
        // login has the same number as the first viewer's
        c.ended(0, 1);
        assert!(c.draft_held(0, JANE));
        c.status(0, 2, 1, true);
        c.batch(0, 2, batch(1, JANE, vec![said(1, "hi again")]));
        assert!(c.draft_held(0, JANE));
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::Held));
        // what the first viewer says late is dropped
        c.batch(0, 1, batch(1, JANE, vec![said(9, "late")]));
        assert_eq!(texts(&c, 0, JANE), ["hi again"]);
    }

    #[test]
    fn a_refusal_stays_in_its_conversation_and_gives_the_text_back() {
        let (mut c, now) = chat(2);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        c.batch(1, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "not allowed"));
        let id = c.submit(0, JANE, now).expect("sent");
        sends(&mut c);
        // an answer from another character's viewer is not this line's
        c.result(1, 1, id, SendOutcome::Accepted);
        assert_eq!(c.sending(0, JANE), 1);
        c.result(0, 1, id, SendOutcome::Refused(SendRefusal::Restricted));
        assert_eq!(texts(&c, 0, JANE), ["hi", "<refused Restricted: not allowed>"]);
        assert_eq!(texts(&c, 1, JANE), ["hi"]);
        assert_eq!(c.draft(0, JANE), "not allowed");
        assert_eq!(c.draft(1, JANE), "");
        assert!(sends(&mut c).is_empty(), "never tried again, as anyone");
        // a second answer changes nothing
        c.result(0, 1, id, SendOutcome::Accepted);
        assert_eq!(texts(&c, 0, JANE).len(), 2);
    }

    #[test]
    fn a_line_refused_as_stale_comes_back_held() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "crossed a relogin"));
        let id = c.submit(0, JANE, now).expect("sent");
        // the viewer logged out and in before the line reached it
        c.status(0, 1, 3, true);
        c.result(0, 1, id, SendOutcome::Refused(SendRefusal::Stale));
        assert_eq!(c.draft(0, JANE), "crossed a relogin");
        assert!(c.draft_held(0, JANE));
    }

    #[test]
    fn a_line_without_an_answer_is_uncertain_and_never_sent_again() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        // the viewer's process ends before it answers
        assert!(c.set_draft(0, JANE, "said or not?"));
        let first = c.submit(0, JANE, now).expect("sent");
        assert_eq!(sends(&mut c).len(), 1);
        c.ended(0, 1);
        assert_eq!(texts(&c, 0, JANE), ["hi", "<uncertain: said or not?>"]);
        assert_eq!(c.draft(0, JANE), "", "not put back: sending it again could say it twice");
        assert_eq!(c.sending(0, JANE), 0);
        // its answer, if it comes after all, changes nothing
        c.result(0, 1, first, SendOutcome::Refused(SendRefusal::Closed));
        assert_eq!(texts(&c, 0, JANE).len(), 2);
        // a viewer that stays silent
        c.status(0, 2, 1, true);
        c.batch(0, 2, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "into the void"));
        c.submit(0, JANE, now).expect("sent");
        c.tick(now + SEND_WAIT - Duration::from_secs(1));
        assert_eq!(c.sending(0, JANE), 1);
        c.tick(now + SEND_WAIT);
        assert_eq!(
            texts(&c, 0, JANE),
            ["<uncertain: said or not?>", "hi", "<uncertain: into the void>"],
            "the new viewer's lines replace the old one's; the notices stay"
        );
        c.tick(now + SEND_WAIT * 3);
        // through all of it, each line was handed to a pipe exactly once
        assert_eq!(sends(&mut c).len(), 1);
    }

    #[test]
    fn a_line_that_never_reached_the_pipe_is_not_sent_and_comes_back() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "the queue was full"));
        let id = c.submit(0, JANE, now).expect("sent");
        c.not_written(id);
        assert_eq!(texts(&c, 0, JANE), ["hi", "<not sent: the queue was full>"]);
        assert_eq!(c.draft(0, JANE), "the queue was full");
        assert!(!c.draft_held(0, JANE));
        // the user sends it again: a new number, never the old one
        let again = c.submit(0, JANE, now).expect("sent");
        assert!(again > id);
        // what the user typed meanwhile is not overwritten by a line given back
        assert!(c.set_draft(0, JANE, "typed meanwhile"));
        c.not_written(again);
        assert_eq!(c.draft(0, JANE), "typed meanwhile");
    }

    #[test]
    fn lines_are_not_sent_from_a_character_out_of_the_world_or_into_nothing() {
        let (mut c, now) = chat(1);
        // no conversation told yet
        assert!(c.set_draft(0, JANE, "hello"));
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::EarlierLogin));
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, " \n "));
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::Empty));
        // a character that never reported has no draft at all
        assert!(!c.set_draft(3, JANE, "hello"));
        assert_eq!(c.can_send(3, JANE), Err(Blocked::NotInWorld));
        assert_eq!(c.can_send(99, JANE), Err(Blocked::NotInWorld));
        // too many unanswered lines
        for n in 0..MAX_PENDING {
            assert!(c.set_draft(0, JANE, &format!("line {n}")));
            c.submit(0, JANE, now).expect("sent");
        }
        assert!(c.set_draft(0, JANE, "one more"));
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::Busy));
        assert_eq!(c.draft(0, JANE), "one more");
        // a typed text is bounded
        let long = "x".repeat(MAX_SEND * 2);
        assert!(c.set_draft(0, GROUP, &long));
        assert_eq!(c.draft(0, GROUP).len(), MAX_SEND);
    }

    #[test]
    fn attention_items_are_listed_per_character_and_only_for_its_login() {
        let (mut c, _) = chat(2);
        let list = |generation| AttentionList {
            generation,
            items: vec![AttentionItem {
                id: 4,
                kind: crate::ipc::AttentionKind::Teleport,
                title: "Jane Doe".into(),
                text: "offers a teleport".into(),
            }],
            more: 2,
        };
        c.attention_list(1, 1, list(1));
        assert_eq!(c.attention(0), (&[][..], 0));
        let (items, more) = c.attention(1);
        assert_eq!((items.len(), more), (1, 2));
        // answered in the viewer: the list empties
        c.attention_list(
            1,
            1,
            AttentionList {
                generation: 1,
                ..Default::default()
            },
        );
        assert_eq!(c.attention(1).0.len(), 0);
        // another login's list, or an old launch's, is not shown
        c.attention_list(1, 1, list(7));
        c.attention_list(1, 0, list(1));
        assert_eq!(c.attention(1).0.len(), 0);
        c.attention_list(1, 1, list(1));
        c.status(1, 1, 2, false);
        assert_eq!(c.attention(1).0.len(), 0, "a logout empties it");
    }

    #[test]
    fn a_removed_character_leaves_nothing_for_the_next_one_in_its_slot() {
        let (mut c, now) = chat(2);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        c.batch(1, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "Sam's words"));
        assert!(c.set_draft(1, JANE, "pending"));
        c.submit(1, JANE, now).expect("sent");
        c.forget(0);
        assert!(c.threads(0).is_empty());
        assert_eq!(c.draft(0, JANE), "");
        assert!(c.feed().all(|f| f.slot == 1));
        assert_eq!(c.sending(1, JANE), 1, "the other characters are untouched");
        // another profile takes the slot: a new launch, nothing of Sam's
        c.status(0, 2, 1, true);
        c.batch(0, 2, batch(1, JANE, vec![said(1, "hi stranger")]));
        assert_eq!(c.draft(0, JANE), "");
        assert_eq!(c.submit(0, JANE, now), Err(Blocked::Empty));
    }

    #[test]
    fn a_conversation_closed_in_the_host_leaves_the_chips_and_nothing_else() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, Conversation::Nearby, vec![said(1, "hello")]));
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi"), said(2, "there")]));
        c.batch(0, 1, batch(1, GROUP, vec![said(1, "meeting")]));
        c.take_effects();
        let open = |c: &Chat| c.open_threads(0).map(|t| t.conversation).collect::<Vec<_>>();
        assert_eq!(open(&c), [Conversation::Nearby, JANE, GROUP]);
        assert!(c.set_draft(0, JANE, "half a line"));
        let feed = c.feed().count();
        // closed: no chip, nothing unread, nothing typed, and not a word
        // to its viewer
        assert!(c.close(0, JANE));
        assert!(!c.close(0, JANE), "closed already");
        assert_eq!(open(&c), [Conversation::Nearby, GROUP]);
        assert!(c.open_thread(0, JANE).is_none());
        assert!(c.thread(0, JANE).is_some_and(Thread::closed));
        assert_eq!(c.unread(0), 2, "the nearby chat's and the group's");
        assert_eq!(c.draft(0, JANE), "");
        assert!(c.take_effects().is_empty());
        assert_eq!(c.feed().count(), feed, "its lines stay in All");
        // the nearby chat is never closed, nor a conversation there is not
        assert!(!c.close(0, Conversation::Nearby));
        assert!(!c.close(0, Conversation::Session(9, 9)));
        assert!(!c.close(MAX_SLOTS, JANE));
        // a line of the user's own, said in the viewer, does not bring it
        // back; a line of someone else does, with the earlier ones
        c.batch(0, 1, batch(1, JANE, vec![line(3, LineKind::Own, "Sam Resident", "bye")]));
        assert!(c.open_thread(0, JANE).is_none());
        c.batch(0, 1, batch(1, JANE, vec![said(4, "wait")]));
        assert_eq!(open(&c), [Conversation::Nearby, JANE, GROUP]);
        assert_eq!(texts(&c, 0, JANE), ["hi", "there", "bye", "wait"]);
        assert_eq!(c.thread(0, JANE).map(|t| t.unread), Some(1));
    }

    #[test]
    fn what_the_host_says_of_a_line_brings_a_closed_conversation_back() {
        let (mut c, now) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.set_draft(0, JANE, "on my way"));
        c.submit(0, JANE, now).expect("sent");
        // closed while the line waits for its answer
        assert!(c.close(0, JANE));
        assert!(c.open_thread(0, JANE).is_none());
        // no answer: uncertain, and the user is told where they typed it
        c.tick(now + SEND_WAIT);
        assert!(c.open_thread(0, JANE).is_some());
        assert!(matches!(
            c.thread(0, JANE).and_then(|t| t.entries().last()),
            Some(Entry::Note(Note::Uncertain { .. }))
        ));
    }

    #[test]
    fn a_pin_is_the_host_s_and_lasts_for_one_login() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        c.batch(0, 1, batch(1, GROUP, vec![said(1, "meeting")]));
        c.take_effects();
        assert!(c.set_pinned(0, GROUP, true));
        assert!(!c.set_pinned(0, GROUP, true), "pinned already");
        assert!(!c.set_pinned(0, Conversation::Session(9, 9), true));
        assert!(c.thread(0, GROUP).is_some_and(Thread::pinned));
        assert!(c.take_effects().is_empty(), "nothing is said to the viewer");
        // the more recent conversation is the one with the later batch
        let (jane, group) = (c.thread(0, JANE).expect("jane"), c.thread(0, GROUP).expect("group"));
        assert!(group.recency() > jane.recency());
        c.batch(0, 1, batch(1, JANE, vec![said(2, "again")]));
        let (jane, group) = (c.thread(0, JANE).expect("jane"), c.thread(0, GROUP).expect("group"));
        assert!(jane.recency() > group.recency());
        // closing takes the pin
        assert!(c.close(0, GROUP));
        assert!(!c.thread(0, GROUP).is_some_and(Thread::pinned));
        assert!(!c.set_pinned(0, GROUP, true), "a closed conversation is not pinned");
        // another login: the pins are gone
        assert!(c.set_pinned(0, JANE, true));
        c.status(0, 1, 2, true);
        assert!(!c.thread(0, JANE).is_some_and(Thread::pinned));
    }

    #[test]
    fn what_any_window_shows_with_the_keyboard_is_read() {
        let (mut c, _) = chat(2);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "hi")]));
        c.batch(1, 1, batch(1, GROUP, vec![said(1, "meeting")]));
        c.take_effects();
        assert_eq!((c.unread(0), c.unread(1)), (1, 1));
        // a monitor's window shows the second character's conversation,
        // without the keyboard: not read
        c.look(Viewing::All, false);
        c.look_from(2, Viewing::Thread(1, GROUP), false);
        assert_eq!((c.unread(0), c.unread(1)), (1, 1));
        // it gets the keyboard: read there, and nowhere else
        c.look_from(2, Viewing::Thread(1, GROUP), true);
        assert_eq!((c.unread(0), c.unread(1)), (1, 0));
        // what comes while it is on screen there is read as it comes,
        // although the shared chat's window has not the keyboard
        c.batch(1, 1, batch(1, GROUP, vec![said(2, "still there?")]));
        c.batch(0, 1, batch(1, JANE, vec![said(2, "hello?")]));
        assert_eq!((c.unread(0), c.unread(1)), (2, 0));
        // the window closed (it shows nothing): what comes is unread again
        c.look_from(2, Viewing::Nothing, false);
        c.batch(1, 1, batch(1, GROUP, vec![said(3, "anyone?")]));
        assert_eq!(c.unread(1), 1);
        // the shared chat's window is one of them, as before
        c.look(Viewing::All, true);
        assert_eq!((c.unread(0), c.unread(1)), (0, 0));
    }

    #[test]
    fn a_conversation_on_screen_in_any_window_is_not_the_one_dropped() {
        let (mut c, _) = chat(1);
        c.batch(0, 1, batch(1, JANE, vec![said(1, "first of all")]));
        // on screen in a monitor's window, without the keyboard
        c.look_from(3, Viewing::Thread(0, JANE), false);
        for n in 0..MAX_THREADS as u64 + 4 {
            c.batch(0, 1, batch(1, Conversation::Session(1, 100 + n), vec![said(1, "x")]));
        }
        assert!(c.threads(0).len() <= MAX_THREADS);
        assert!(c.thread(0, JANE).is_some(), "the oldest, but on screen");
    }

    #[test]
    fn a_batch_that_is_not_one_is_dropped() {
        let (mut c, _) = chat(1);
        let mut wrong = batch(1, JANE, vec![said(1, "hi")]);
        wrong.kind = SessionKind::Nearby;
        c.batch(0, 1, wrong);
        let mut wrong = batch(1, Conversation::Nearby, vec![said(1, "hi")]);
        wrong.kind = SessionKind::Group;
        c.batch(0, 1, wrong);
        c.batch(MAX_SLOTS, 1, batch(1, JANE, vec![said(1, "hi")]));
        assert!(c.threads(0).is_empty());
        assert_eq!(c.feed().count(), 0);
        // what a viewer sends is cleaned before it is kept
        c.batch(0, 1, batch(1, JANE, vec![said(1, "a\u{7}b\u{0}")]));
        assert_eq!(texts(&c, 0, JANE), ["ab"]);
    }
}
