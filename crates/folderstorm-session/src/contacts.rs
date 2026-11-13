//! Each character's friends and groups in the host, and the conversations
//! the host asks a viewer to open (session-host plan, checkpoint 11).
//! Pure: it is told what came from a viewer, what the user did and the
//! time, and answers with the messages for the host to write.
//!
//! The rules it keeps:
//! - **The viewer's list, its login only.** A character's friends and
//!   groups are what its viewer told for the login it is in; a list of
//!   another launch or login is dropped, and a logout empties it. Nothing
//!   of it is written anywhere: it lives as long as that login.
//! - **The viewer decides.** "New IM" asks the character's viewer to open
//!   the conversation as the user would there; the viewer applies its own
//!   rules (RLVa's `@startim` and `@startimto`, its groups) and answers.
//!   A refusal, or no answer, comes back as a notice in the window that
//!   asked. Nothing is ever said to anyone by opening: a line is said only
//!   when the user types one (the chat's own rules, `chat`).
//! - **Opened, then shown.** A conversation the viewer opened is shown in
//!   the host at once ([`Chat::opened`]), in the window that asked for it.
//! - **One request, one answer.** Each request has a number that grows;
//!   only the first answer of the launch it was written to counts, and a
//!   request without an answer after [`OPEN_WAIT`] is told as such, never
//!   written again.

use crate::chat::Chat;
use crate::ipc::{Contact, Contacts, Conversation, MAX_SLOTS, Open, OpenOutcome, OpenRefusal, SessionKind, ToViewer};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// A request not answered after this long is told as not answered.
pub const OPEN_WAIT: Duration = Duration::from_secs(15);
/// Requests of one character that may wait for their answer at once.
pub const MAX_OPENING: usize = 4;

/// One login of one launch of a slot's viewer.
type Login = (u32, u32);

/// What a search lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Which {
    /// The friends, online first.
    #[default]
    Friends,
    /// "New IM": the groups, then the friends.
    Everyone,
}

/// One row of a search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found<'a> {
    pub contact: &'a Contact,
    pub group: bool,
}

impl Found<'_> {
    /// What a click on the row asks the viewer to open.
    pub fn open(&self) -> Open {
        if self.group {
            Open::Group(self.contact.who)
        } else {
            Open::Im(self.contact.who)
        }
    }
}

/// Why a request was not written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// The character has no login (not in the world, no viewer).
    NotInWorld,
    /// Too many requests of this character wait for their answer.
    Busy,
    /// It names nobody.
    Nobody,
}

/// What became of a request, told in the window that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Refused(OpenRefusal),
    /// Written, and no answer came ([`OPEN_WAIT`], or the viewer ended).
    NoAnswer,
    /// The viewer's pipe did not take it.
    NotSent,
}

/// A notice of the host about a request of the user's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub slot: usize,
    /// Who or what was asked for, as the list named it.
    pub name: String,
    pub open: Open,
    pub why: Why,
}

/// A conversation the viewer opened: the window that asked shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opened {
    pub window: u8,
    pub slot: usize,
    pub conversation: Conversation,
}

/// A message for the host to write on that launch's pipe, and no other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Send {
    pub slot: usize,
    pub launch: u32,
    pub id: u64,
    pub message: ToViewer,
}

#[derive(Clone, Debug)]
struct Pending {
    id: u64,
    slot: usize,
    launch: u32,
    generation: u32,
    window: u8,
    open: Open,
    name: String,
    at: Instant,
}

#[derive(Clone, Debug)]
struct List {
    login: Login,
    contacts: Contacts,
}

/// Whether every word of `query` is in `name`, whatever their case.
pub fn matches(name: &str, query: &str) -> bool {
    let name = name.to_lowercase();
    query.split_whitespace().all(|word| name.contains(&word.to_lowercase()))
}

/// The friends in the order the lists show them: online first, then by
/// name whatever its case, then by id (two of one name keep their place).
pub fn sort_friends(friends: &mut [Contact]) {
    friends.sort_by_cached_key(|c| (!c.online, c.name.to_lowercase(), c.who));
}

/// The groups by name.
pub fn sort_groups(groups: &mut [Contact]) {
    groups.sort_by_cached_key(|c| (c.name.to_lowercase(), c.who));
}

/// The characters' friends and groups, and the requests to open.
#[derive(Debug)]
pub struct Book {
    lists: [Option<List>; MAX_SLOTS],
    pending: Vec<Pending>,
    next_id: u64,
    notices: BTreeMap<u8, Notice>,
    opened: Vec<Opened>,
    effects: Vec<Send>,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            lists: Default::default(),
            pending: Vec::new(),
            next_id: 1,
            notices: BTreeMap::new(),
            opened: Vec::new(),
            effects: Vec::new(),
        }
    }
}

impl Book {
    /// A list from `launch` of the slot's viewer; kept when it is of the
    /// login the character is in (`login`: the chat's, `Chat::login`), or
    /// when the chat does not know that login yet (the list came first:
    /// [`Book::sync`] drops it if the login told is another one).
    pub fn set(&mut self, slot: usize, launch: u32, login: Option<Login>, contacts: Contacts) {
        if slot >= MAX_SLOTS || login.is_some_and(|login| login != (launch, contacts.generation)) {
            return;
        }
        let mut contacts = contacts.cleaned();
        sort_friends(&mut contacts.friends);
        sort_groups(&mut contacts.groups);
        self.lists[slot] = Some(List {
            login: (launch, contacts.generation),
            contacts,
        });
    }

    /// The character's login now: a list of another one is dropped. (No
    /// login known keeps it: the viewer ending empties it, [`Book::ended`].)
    pub fn sync(&mut self, slot: usize, login: Option<Login>) {
        if let Some(list) = self.lists.get_mut(slot)
            && list.as_ref().is_some_and(|l| login.is_some_and(|login| login != l.login))
        {
            *list = None;
        }
    }

    /// The slot holds no character any more, or another one: nothing of
    /// it stays.
    pub fn forget(&mut self, slot: usize) {
        if let Some(list) = self.lists.get_mut(slot) {
            *list = None;
        }
        self.pending.retain(|p| p.slot != slot);
        self.notices.retain(|_, n| n.slot != slot);
        self.opened.retain(|o| o.slot != slot);
    }

    /// Whether the character's viewer told a list for this login.
    pub fn known(&self, slot: usize) -> bool {
        self.lists.get(slot).is_some_and(Option::is_some)
    }

    fn contacts(&self, slot: usize) -> Option<&Contacts> {
        self.lists.get(slot)?.as_ref().map(|l| &l.contacts)
    }

    /// The friends, online first.
    pub fn friends(&self, slot: usize) -> &[Contact] {
        self.contacts(slot).map_or(&[], |c| c.friends.as_slice())
    }

    /// The groups, by name.
    pub fn groups(&self, slot: usize) -> &[Contact] {
        self.contacts(slot).map_or(&[], |c| c.groups.as_slice())
    }

    /// How many friends and groups the viewer did not tell (they are in
    /// its own lists).
    pub fn more(&self, slot: usize) -> (u32, u32) {
        self.contacts(slot).map_or((0, 0), |c| (c.more_friends, c.more_groups))
    }

    /// How many friends are online.
    pub fn online(&self, slot: usize) -> usize {
        self.friends(slot).iter().filter(|c| c.online).count()
    }

    /// The rows a search lists, in their order.
    pub fn search(&self, slot: usize, query: &str, which: Which) -> Vec<Found<'_>> {
        let friends = self.friends(slot).iter().map(|contact| Found { contact, group: false });
        let groups = self.groups(slot).iter().map(|contact| Found { contact, group: true });
        let rows: Vec<Found<'_>> = match which {
            Which::Friends => friends.collect(),
            Which::Everyone => groups.chain(friends).collect(),
        };
        rows.into_iter().filter(|f| matches(&f.contact.name, query)).collect()
    }

    /// Asks the character's viewer to open something, for the login it is
    /// in (`login`), from `window`. The message is in
    /// [`Book::take_effects`]; one the pipe does not take is told back
    /// with [`Book::not_written`].
    pub fn open(&mut self, slot: usize, login: Option<Login>, window: u8, open: Open, name: &str, now: Instant) -> Result<u64, Blocked> {
        let Some((launch, generation)) = login.filter(|_| slot < MAX_SLOTS) else {
            return Err(Blocked::NotInWorld);
        };
        if open.target().is_none() {
            return Err(Blocked::Nobody);
        }
        if self.pending.iter().filter(|p| p.slot == slot).count() >= MAX_OPENING {
            return Err(Blocked::Busy);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.notices.remove(&window);
        self.pending.push(Pending {
            id,
            slot,
            launch,
            generation,
            window,
            open,
            name: crate::ipc::clean(name, crate::ipc::MAX_NAME),
            at: now,
        });
        self.effects.push(Send {
            slot,
            launch,
            id,
            message: ToViewer::Open { id, generation, open },
        });
        Ok(id)
    }

    /// Whether a request for `open` of that character waits for its answer.
    pub fn opening(&self, slot: usize, open: Open) -> bool {
        self.pending.iter().any(|p| p.slot == slot && p.open == open)
    }

    fn tell(&mut self, p: &Pending, why: Why) {
        self.notices.insert(
            p.window,
            Notice {
                slot: p.slot,
                name: p.name.clone(),
                open: p.open,
                why,
            },
        );
    }

    /// The viewer's answer to request `id`. Only the first answer of the
    /// launch it was written to counts. A conversation opened is shown in
    /// the chat, for that login.
    pub fn answer(&mut self, slot: usize, launch: u32, id: u64, outcome: OpenOutcome, chat: &mut Chat) {
        let Some(at) = self.pending.iter().position(|p| p.id == id && p.slot == slot && p.launch == launch) else {
            return;
        };
        let p = self.pending.remove(at);
        match outcome {
            OpenOutcome::Refused(reason) => self.tell(&p, Why::Refused(reason)),
            OpenOutcome::Opened => {
                let kind = match p.open {
                    Open::Group(_) => SessionKind::Group,
                    Open::Im(_) | Open::Profile(_) => SessionKind::Resident,
                };
                if let Some(conversation) = p.open.conversation()
                    && chat.opened(slot, launch, p.generation, conversation, kind, &p.name)
                {
                    self.opened.push(Opened {
                        window: p.window,
                        slot,
                        conversation,
                    });
                }
            }
        }
    }

    /// Request `id` never reached its viewer's pipe.
    pub fn not_written(&mut self, id: u64) {
        if let Some(at) = self.pending.iter().position(|p| p.id == id) {
            let p = self.pending.remove(at);
            self.tell(&p, Why::NotSent);
        }
    }

    /// That launch of the slot's viewer ended, or its link did: what it
    /// did not answer is told as not answered.
    pub fn ended(&mut self, slot: usize, launch: u32) {
        let (lost, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| p.slot == slot && p.launch == launch);
        self.pending = kept;
        for p in lost {
            self.tell(&p, Why::NoAnswer);
        }
        if let Some(list) = self.lists.get_mut(slot)
            && list.as_ref().is_some_and(|l| l.login.0 == launch)
        {
            *list = None;
        }
    }

    /// Now and then: requests without an answer for [`OPEN_WAIT`].
    pub fn tick(&mut self, now: Instant) {
        let (late, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| now.saturating_duration_since(p.at) >= OPEN_WAIT);
        self.pending = kept;
        for p in late {
            self.tell(&p, Why::NoAnswer);
        }
    }

    /// What the host says to `window` of its last request, until dismissed
    /// or the next request.
    pub fn notice(&self, window: u8) -> Option<&Notice> {
        self.notices.get(&window)
    }

    pub fn dismiss(&mut self, window: u8) {
        self.notices.remove(&window);
    }

    /// The conversations opened since the last call, for the windows that
    /// asked.
    pub fn take_opened(&mut self) -> Vec<Opened> {
        std::mem::take(&mut self.opened)
    }

    pub fn take_effects(&mut self) -> Vec<Send> {
        std::mem::take(&mut self.effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED: u8 = crate::chat::SHARED_WINDOW;
    const MONITOR: u8 = 3;

    fn contact(n: u128, name: &str, online: bool) -> Contact {
        Contact {
            who: Conversation::session(n),
            name: name.to_owned(),
            online,
        }
    }

    fn list(generation: u32) -> Contacts {
        Contacts {
            generation,
            friends: vec![
                contact(1, "zoe Resident", false),
                contact(2, "Adam Resident", false),
                contact(3, "Mia Resident", true),
                contact(4, "bob Resident", true),
            ],
            groups: vec![contact(10, "Plaza Market", false), contact(11, "Aurora Builders", false)],
            more_friends: 2,
            more_groups: 0,
        }
    }

    /// A chat and a book with one character in the world, launch 1, login 1.
    fn book() -> (Book, Chat, Instant) {
        let mut chat = Chat::default();
        chat.status(0, 1, 1, true);
        let mut book = Book::default();
        book.set(0, 1, chat.login(0), list(1));
        (book, chat, Instant::now())
    }

    fn names(found: &[Found<'_>]) -> Vec<String> {
        found.iter().map(|f| f.contact.name.clone()).collect()
    }

    #[test]
    fn friends_are_online_first_then_by_name_and_groups_by_name() {
        let (book, _, _) = book();
        assert_eq!(
            names(&book.search(0, "", Which::Friends)),
            ["bob Resident", "Mia Resident", "Adam Resident", "zoe Resident"]
        );
        assert_eq!(book.online(0), 2);
        assert_eq!(book.more(0), (2, 0));
        // "New IM": the groups first, then the friends
        let everyone = book.search(0, "", Which::Everyone);
        assert_eq!(names(&everyone[..2]), ["Aurora Builders", "Plaza Market"]);
        assert!(everyone[0].group && !everyone[2].group);
        assert_eq!(everyone[0].open(), Open::Group(Conversation::session(11)));
        assert_eq!(everyone[2].open(), Open::Im(Conversation::session(4)));
    }

    #[test]
    fn a_search_takes_every_word_whatever_its_case() {
        let (book, _, _) = book();
        assert_eq!(names(&book.search(0, "RES  mi", Which::Friends)), ["Mia Resident"]);
        assert_eq!(names(&book.search(0, "E", Which::Everyone)).len(), 6, "every name has an e");
        assert!(book.search(0, "nobody", Which::Everyone).is_empty());
        assert!(matches("Élodie Résident", "élodie"));
        assert!(matches("anything", "   "));
        assert!(!matches("Jane", "jane doe"));
    }

    #[test]
    fn a_list_is_the_login_s_and_goes_with_it() {
        let (mut book, mut chat, _) = book();
        assert!(book.known(0));
        // a list of an earlier launch or login is dropped
        let mut other = Book::default();
        other.set(0, 2, chat.login(0), list(1));
        other.set(0, 1, chat.login(0), list(2));
        assert!(!other.known(0));
        // a logout empties it; another character's is its own
        chat.status(0, 1, 2, true);
        book.sync(0, chat.login(0));
        assert!(!book.known(0) && book.friends(0).is_empty());
        assert!(book.search(0, "", Which::Everyone).is_empty());
        book.set(0, 1, chat.login(0), list(2));
        assert!(book.known(0));
        book.ended(0, 1);
        assert!(!book.known(0), "the viewer ended: its list goes");
        book.set(0, 1, Some((1, 2)), list(2));
        book.forget(0);
        assert!(!book.known(0));
        // a list that comes before the chat knows the login is kept, and
        // dropped if the login told is another one
        book.set(0, 3, None, list(1));
        assert!(book.known(0));
        book.sync(0, None);
        assert!(book.known(0));
        book.sync(0, Some((3, 1)));
        assert!(book.known(0), "its own login");
        book.sync(0, Some((3, 2)));
        assert!(!book.known(0), "another login");
    }

    #[test]
    fn opening_asks_the_viewer_and_shows_the_conversation_once_it_says_so() {
        let (mut book, mut chat, now) = book();
        let mia = Conversation::session(3);
        let id = book
            .open(0, chat.login(0), MONITOR, Open::Im(mia), "Mia Resident", now)
            .expect("asked");
        assert_eq!(
            book.take_effects(),
            [Send {
                slot: 0,
                launch: 1,
                id,
                message: ToViewer::Open {
                    id,
                    generation: 1,
                    open: Open::Im(mia)
                }
            }]
        );
        assert!(book.opening(0, Open::Im(mia)));
        assert!(chat.thread(0, mia).is_none(), "nothing shown before the viewer opened it");
        // an answer from another launch, or of another number, is not it
        book.answer(0, 2, id, OpenOutcome::Opened, &mut chat);
        book.answer(0, 1, id + 1, OpenOutcome::Opened, &mut chat);
        assert!(chat.thread(0, mia).is_none());
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        let thread = chat.thread(0, mia).expect("shown");
        assert_eq!((thread.kind, thread.title.as_str()), (SessionKind::Resident, "Mia Resident"));
        assert!(!thread.earlier_login, "it can be written to at once");
        assert_eq!(
            book.take_opened(),
            [Opened {
                window: MONITOR,
                slot: 0,
                conversation: mia
            }]
        );
        assert!(!book.opening(0, Open::Im(mia)));
        // nothing was said to anyone: the only message was the request
        assert!(chat.take_effects().is_empty());
        // a second answer counts for nothing
        book.answer(0, 1, id, OpenOutcome::Refused(OpenRefusal::Restricted), &mut chat);
        assert!(book.notice(MONITOR).is_none());
        // a group's chat
        let builders = Conversation::session(11);
        let id = book
            .open(0, chat.login(0), SHARED, Open::Group(builders), "Aurora Builders", now)
            .expect("asked");
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        assert_eq!(chat.thread(0, builders).map(|t| t.kind), Some(SessionKind::Group));
        // a profile opens no conversation
        let id = book
            .open(0, chat.login(0), SHARED, Open::Profile(mia), "Mia Resident", now)
            .expect("asked");
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        assert_eq!(book.take_opened().len(), 1, "the group's, not the profile's");
    }

    #[test]
    fn a_conversation_closed_in_the_host_or_of_an_earlier_login_comes_back_clean() {
        let (mut book, mut chat, now) = book();
        let mia = Conversation::session(3);
        let id = book.open(0, chat.login(0), SHARED, Open::Im(mia), "Mia", now).expect("asked");
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        assert!(chat.close(0, mia));
        assert!(chat.open_thread(0, mia).is_none());
        let id = book.open(0, chat.login(0), SHARED, Open::Im(mia), "Mia", now).expect("asked");
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        assert!(chat.open_thread(0, mia).is_some(), "opened again: back among the chips");
        // the next login: the conversation is of the earlier one until opened
        chat.status(0, 1, 2, true);
        assert!(chat.thread(0, mia).is_some_and(|t| t.earlier_login));
        let id = book.open(0, chat.login(0), SHARED, Open::Im(mia), "Mia", now).expect("asked");
        book.answer(0, 1, id, OpenOutcome::Opened, &mut chat);
        assert!(chat.thread(0, mia).is_some_and(|t| !t.earlier_login && t.entries().len() == 0));
        assert!(
            chat.can_send(0, mia).is_err_and(|b| b == crate::chat::Blocked::Empty),
            "nothing typed yet"
        );
    }

    #[test]
    fn a_refusal_or_no_answer_is_a_notice_in_the_window_that_asked() {
        let (mut book, mut chat, now) = book();
        let bob = Conversation::session(4);
        let id = book
            .open(0, chat.login(0), MONITOR, Open::Im(bob), "bob Resident", now)
            .expect("asked");
        book.answer(0, 1, id, OpenOutcome::Refused(OpenRefusal::Restricted), &mut chat);
        assert!(chat.thread(0, bob).is_none());
        let notice = book.notice(MONITOR).expect("told");
        assert_eq!(
            (notice.slot, notice.name.as_str(), notice.why),
            (0, "bob Resident", Why::Refused(OpenRefusal::Restricted))
        );
        assert!(book.notice(SHARED).is_none(), "in the window that asked only");
        // the next request there takes its place; dismissing clears it
        let id = book
            .open(0, chat.login(0), MONITOR, Open::Im(bob), "bob Resident", now)
            .expect("asked");
        assert!(book.notice(MONITOR).is_none());
        book.not_written(id);
        assert_eq!(book.notice(MONITOR).map(|n| n.why), Some(Why::NotSent));
        book.dismiss(MONITOR);
        assert!(book.notice(MONITOR).is_none());
        // written and never answered
        book.open(0, chat.login(0), SHARED, Open::Im(bob), "bob Resident", now)
            .expect("asked");
        book.tick(now + OPEN_WAIT - Duration::from_millis(1));
        assert!(book.notice(SHARED).is_none());
        book.tick(now + OPEN_WAIT);
        assert_eq!(book.notice(SHARED).map(|n| n.why), Some(Why::NoAnswer));
        // the viewer ended with a request waiting
        book.open(0, chat.login(0), MONITOR, Open::Im(bob), "bob Resident", now)
            .expect("asked");
        book.ended(0, 1);
        assert_eq!(book.notice(MONITOR).map(|n| n.why), Some(Why::NoAnswer));
    }

    #[test]
    fn nothing_is_asked_without_a_login_for_nobody_or_too_often() {
        let (mut book, chat, now) = book();
        let bob = Open::Im(Conversation::session(4));
        assert_eq!(book.open(1, chat.login(1), SHARED, bob, "bob", now), Err(Blocked::NotInWorld));
        assert_eq!(
            book.open(0, chat.login(0), SHARED, Open::Im(Conversation::Nearby), "", now),
            Err(Blocked::Nobody)
        );
        assert_eq!(
            book.open(0, chat.login(0), SHARED, Open::Group(Conversation::Session(0, 0)), "", now),
            Err(Blocked::Nobody)
        );
        for _ in 0..MAX_OPENING {
            book.open(0, chat.login(0), SHARED, bob, "bob", now).expect("asked");
        }
        assert_eq!(book.open(0, chat.login(0), SHARED, bob, "bob", now), Err(Blocked::Busy));
        assert_eq!(book.take_effects().len(), MAX_OPENING);
        // numbers grow, whoever they are for
        book.forget(0);
        let mut chat = chat;
        chat.status(1, 1, 1, true);
        let id = book.open(1, chat.login(1), SHARED, bob, "bob", now).expect("asked");
        assert_eq!(id, MAX_OPENING as u64 + 1);
    }
}
