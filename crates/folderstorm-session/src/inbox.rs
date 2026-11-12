//! Checkpoint 7 of the session host, the attention inbox (session-host
//! plan, section 13): one list, across all characters, of what waits for
//! an answer in their own windows (offers, teleport and friendship
//! requests, scripts' dialogs and permission questions, calls: whatever a
//! viewer lists as [`AttentionItem`]s, checkpoint 3).
//!
//! The rules, all pure:
//! - **Nothing is answered here.** The inbox has no way to accept, decline
//!   or dismiss: it says which character is asked what and for how long,
//!   and the host offers "Switch to answer". The answer is given in that
//!   character's own window.
//! - **The viewers' lists are the truth.** A question is in the inbox
//!   while its viewer lists it, and leaves when the viewer's next list
//!   does not have it (answered there, or expired there), when that
//!   character logs out or in again, and when its viewer ends or is let
//!   go. The host itself expires nothing: a question shown here still
//!   waits there.
//! - **Each question once.** A question is one login's number of one
//!   character's viewer: listed twice, it is one entry; another login's
//!   same number is another question.
//! - **The age is the host's.** A question's age counts from when the
//!   host first saw it listed, and is kept while it stays listed.
//! - **Oldest first.** What waited the longest is the nearest to expiring
//!   in its viewer; then by character, then by the viewer's number.
//! - **Bounded.** [`MAX_ATTENTION`] questions per character, so
//!   [`MAX_ENTRIES`] in all; what a viewer has beyond that is counted,
//!   not listed.

use crate::ipc::{AttentionItem, MAX_ATTENTION, MAX_SLOTS};
use std::time::{Duration, Instant};

/// The most questions the inbox lists.
pub const MAX_ENTRIES: usize = MAX_SLOTS * MAX_ATTENTION;

/// One login of one launch of a slot's viewer.
pub type Login = (u32, u32);

/// One question waiting in a character's own window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub slot: usize,
    pub item: AttentionItem,
    /// When the host first saw it listed.
    pub since: Instant,
}

impl Waiting {
    /// For how long the host has seen it wait.
    pub fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.since)
    }
}

/// An age as the inbox words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    /// Less than a minute.
    Now,
    Minutes(u32),
    Hours(u32),
}

impl Age {
    pub fn of(age: Duration) -> Self {
        let minutes = age.as_secs() / 60;
        match minutes {
            0 => Age::Now,
            1..60 => Age::Minutes(minutes as u32),
            _ => Age::Hours(u32::try_from(minutes / 60).unwrap_or(u32::MAX)),
        }
    }
}

/// The attention inbox.
#[derive(Clone, Debug, Default)]
pub struct Inbox {
    /// Oldest first.
    waiting: Vec<Waiting>,
    logins: [Option<Login>; MAX_SLOTS],
    /// How many more wait in each character's viewer than are listed.
    more: [u32; MAX_SLOTS],
}

impl Inbox {
    /// What that character's viewer lists now, whole: `login` is the login
    /// the list is of (None: not logged in, no viewer, or not managed),
    /// `more` how many more wait there than `items` tells. True when the
    /// inbox changed.
    pub fn set(&mut self, slot: usize, login: Option<Login>, items: &[AttentionItem], more: u32, now: Instant) -> bool {
        if slot >= MAX_SLOTS {
            return false;
        }
        let before = (self.waiting.len(), self.more[slot]);
        // another login's questions are other questions, whatever their
        // numbers
        let same_login = login.is_some() && self.logins[slot] == login;
        self.logins[slot] = login;
        let (mut old, mut kept): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut self.waiting).into_iter().partition(|w| w.slot == slot);
        if !same_login {
            old.clear();
        }
        let mut listed = 0usize;
        let mut over = 0u32;
        let mut replaced = false;
        if login.is_some() {
            for item in items {
                if kept.iter().rev().take(listed).any(|w| w.item.id == item.id) {
                    // listed twice: one question
                    continue;
                }
                if listed == MAX_ATTENTION {
                    over = over.saturating_add(1);
                    continue;
                }
                let known = old.iter().find(|w| w.item.id == item.id);
                replaced |= known.is_none_or(|w| w.item != *item);
                kept.push(Waiting {
                    slot,
                    item: item.clone(),
                    since: known.map_or(now, |w| w.since),
                });
                listed += 1;
            }
            self.more[slot] = more.saturating_add(over);
        } else {
            self.more[slot] = 0;
        }
        let gone = old.len() != listed;
        kept.sort_by_key(|w| (w.since, w.slot, w.item.id));
        self.waiting = kept;
        replaced || gone || before != (self.waiting.len(), self.more[slot])
    }

    /// Nothing of that character stays (removed from the desk).
    pub fn clear(&mut self, slot: usize) {
        if slot < MAX_SLOTS {
            self.waiting.retain(|w| w.slot != slot);
            self.logins[slot] = None;
            self.more[slot] = 0;
        }
    }

    /// What waits, oldest first.
    pub fn waiting(&self) -> &[Waiting] {
        &self.waiting
    }

    /// How many more wait in that character's viewer than are listed.
    pub fn more(&self, slot: usize) -> u32 {
        self.more.get(slot).copied().unwrap_or(0)
    }

    /// How many wait for that character, those not listed included.
    pub fn count_for(&self, slot: usize) -> u32 {
        let listed = self.waiting.iter().filter(|w| w.slot == slot).count();
        u32::try_from(listed).unwrap_or(u32::MAX).saturating_add(self.more(slot))
    }

    /// How many wait in all: the inbox's badge.
    pub fn count(&self) -> u32 {
        (0..MAX_SLOTS).fold(0, |sum, slot| sum.saturating_add(self.count_for(slot)))
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Chat;
    use crate::ipc::{AttentionKind, AttentionList};

    const LOGIN: Option<Login> = Some((1, 1));

    fn item(id: u64, title: &str) -> AttentionItem {
        AttentionItem {
            id,
            kind: AttentionKind::Teleport,
            title: title.into(),
            text: "Join me".into(),
        }
    }

    fn order(inbox: &Inbox) -> Vec<(usize, u64)> {
        inbox.waiting().iter().map(|w| (w.slot, w.item.id)).collect()
    }

    #[test]
    fn the_oldest_question_is_first_across_characters() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut inbox = Inbox::default();
        assert!(inbox.is_empty());
        assert!(inbox.set(2, LOGIN, &[item(7, "Jane")], 0, at(0)));
        assert!(inbox.set(0, LOGIN, &[item(3, "Sam")], 0, at(10)));
        assert!(inbox.set(2, LOGIN, &[item(7, "Jane"), item(9, "Kim")], 0, at(20)));
        assert_eq!(order(&inbox), [(2, 7), (0, 3), (2, 9)]);
        // seen at the same instant: by character, then by number
        let mut inbox = Inbox::default();
        inbox.set(1, LOGIN, &[item(5, "b"), item(4, "a")], 0, at(0));
        inbox.set(0, LOGIN, &[item(9, "c")], 0, at(0));
        assert_eq!(order(&inbox), [(0, 9), (1, 4), (1, 5)]);
        assert_eq!((inbox.count(), inbox.count_for(1), inbox.count_for(4)), (3, 2, 0));
    }

    #[test]
    fn a_question_keeps_its_age_while_it_stays_listed() {
        let start = Instant::now();
        let later = start + Duration::from_secs(90);
        let mut inbox = Inbox::default();
        inbox.set(0, LOGIN, &[item(1, "Jane")], 0, start);
        assert!(!inbox.set(0, LOGIN, &[item(1, "Jane")], 0, later), "the same list changes nothing");
        assert_eq!(inbox.waiting()[0].since, start);
        assert_eq!(inbox.waiting()[0].age(later), Duration::from_secs(90));
        // (a clock that went back gives no negative age)
        assert_eq!(inbox.waiting()[0].age(start - Duration::from_secs(1)), Duration::ZERO);
        // its words changed there: the same question, the same age
        assert!(inbox.set(0, LOGIN, &[item(1, "Jane Doe")], 0, later));
        assert_eq!(inbox.waiting()[0].since, start);
        assert_eq!(inbox.waiting()[0].item.title, "Jane Doe");
    }

    #[test]
    fn a_question_listed_twice_is_one_entry() {
        let now = Instant::now();
        let mut inbox = Inbox::default();
        inbox.set(
            0,
            LOGIN,
            &[item(1, "Jane"), item(1, "Jane"), item(2, "Sam"), item(1, "again")],
            0,
            now,
        );
        assert_eq!(order(&inbox), [(0, 1), (0, 2)]);
        assert_eq!(inbox.waiting()[0].item.title, "Jane", "the first one listed");
        assert_eq!(inbox.count(), 2);
        // the same number in another character's viewer is another question
        inbox.set(1, LOGIN, &[item(1, "Kim")], 0, now);
        assert_eq!(order(&inbox), [(0, 1), (0, 2), (1, 1)]);
    }

    #[test]
    fn what_a_viewer_no_longer_lists_leaves_the_inbox() {
        let start = Instant::now();
        let later = start + Duration::from_secs(30);
        let mut inbox = Inbox::default();
        inbox.set(0, LOGIN, &[item(1, "Jane"), item(2, "Sam")], 0, start);
        inbox.set(1, LOGIN, &[item(1, "Kim")], 0, start);
        // answered or expired in the first character's window
        assert!(inbox.set(0, LOGIN, &[item(2, "Sam")], 0, later));
        assert_eq!(order(&inbox), [(0, 2), (1, 1)]);
        assert_eq!(inbox.waiting()[0].since, start);
        assert!(inbox.set(0, LOGIN, &[], 0, later));
        assert_eq!(order(&inbox), [(1, 1)]);
        assert!(!inbox.set(0, LOGIN, &[], 0, later));
        // asked again there later: a new question, a new age
        inbox.set(0, LOGIN, &[item(2, "Sam")], 0, later);
        assert_eq!(inbox.waiting().iter().find(|w| w.slot == 0).map(|w| w.since), Some(later));
    }

    #[test]
    fn a_logout_a_new_login_and_an_ended_viewer_empty_that_character_s_part() {
        let start = Instant::now();
        let later = start + Duration::from_secs(30);
        let mut inbox = Inbox::default();
        inbox.set(0, LOGIN, &[item(1, "Jane")], 3, start);
        inbox.set(1, LOGIN, &[item(1, "Kim")], 0, start);
        assert_eq!(inbox.count(), 5);
        // logged out, ended or let go: no login
        assert!(inbox.set(0, None, &[item(1, "Jane")], 3, later));
        assert_eq!((order(&inbox), inbox.count(), inbox.more(0)), (vec![(1, 1)], 1, 0));
        // a new login whose first question has the old one's number
        inbox.set(0, Some((1, 2)), &[item(1, "someone else")], 0, later);
        let again = inbox.waiting().iter().find(|w| w.slot == 0).expect("listed");
        assert_eq!((again.since, again.item.title.as_str()), (later, "someone else"));
        // the same, without a logout seen in between
        inbox.set(1, Some((2, 1)), &[item(1, "after a restart")], 0, later);
        assert!(inbox.waiting().iter().all(|w| w.since == later));
        inbox.clear(0);
        assert_eq!(order(&inbox), [(1, 1)]);
        inbox.clear(MAX_SLOTS + 3);
    }

    #[test]
    fn the_inbox_is_bounded_and_counts_what_it_does_not_list() {
        let now = Instant::now();
        let mut inbox = Inbox::default();
        let many: Vec<AttentionItem> = (0..MAX_ATTENTION as u64 + 5).map(|id| item(id, "x")).collect();
        for slot in 0..MAX_SLOTS {
            inbox.set(slot, LOGIN, &many, 2, now);
        }
        assert_eq!(inbox.waiting().len(), MAX_ENTRIES);
        assert_eq!(inbox.more(0), 7);
        assert_eq!(inbox.count_for(0), MAX_ATTENTION as u32 + 7);
        assert_eq!(inbox.count(), (MAX_SLOTS * (MAX_ATTENTION + 7)) as u32);
        // a slot the desk does not have is not listed
        assert!(!inbox.set(MAX_SLOTS, LOGIN, &many, 0, now));
        assert_eq!(inbox.waiting().len(), MAX_ENTRIES);
        // counts do not overflow
        inbox.set(0, LOGIN, &many, u32::MAX, now);
        inbox.set(1, LOGIN, &many, u32::MAX, now);
        assert_eq!((inbox.more(0), inbox.count()), (u32::MAX, u32::MAX));
    }

    #[test]
    fn an_age_is_worded_in_minutes_then_hours() {
        let age = |seconds: u64| Age::of(Duration::from_secs(seconds));
        assert_eq!(age(0), Age::Now);
        assert_eq!(age(59), Age::Now);
        assert_eq!(age(60), Age::Minutes(1));
        assert_eq!(age(59 * 60 + 59), Age::Minutes(59));
        assert_eq!(age(3600), Age::Hours(1));
        assert_eq!(age(3 * 3600 + 1800), Age::Hours(3));
        assert_eq!(Age::of(Duration::MAX), Age::Hours(u32::MAX));
    }

    /// The inbox as the host fills it: from what the chat keeps of each
    /// viewer's list.
    #[test]
    fn the_inbox_follows_the_chat_s_attention_lists() {
        let start = Instant::now();
        let later = start + Duration::from_secs(120);
        let mut chat = Chat::default();
        let mut inbox = Inbox::default();
        let sync = |chat: &Chat, inbox: &mut Inbox, now: Instant| {
            for slot in 0..MAX_SLOTS {
                let (items, more) = chat.attention(slot);
                inbox.set(slot, chat.login(slot), items, more, now);
            }
        };
        chat.status(0, 1, 1, true);
        chat.status(1, 1, 1, true);
        let list = |items: Vec<AttentionItem>| AttentionList {
            generation: 1,
            items,
            more: 0,
        };
        chat.attention_list(1, 1, list(vec![item(4, "Kim")]));
        sync(&chat, &mut inbox, start);
        chat.attention_list(0, 1, list(vec![item(1, "Jane")]));
        sync(&chat, &mut inbox, later);
        assert_eq!(order(&inbox), [(1, 4), (0, 1)]);
        assert_eq!(inbox.waiting()[0].age(later), Duration::from_secs(120));
        // an earlier launch's list is not taken, and changes nothing
        chat.attention_list(0, 0, list(vec![item(9, "stale")]));
        sync(&chat, &mut inbox, later);
        assert_eq!(order(&inbox), [(1, 4), (0, 1)]);
        // the first character's viewer ends: its questions went with it
        chat.ended(0, 1);
        sync(&chat, &mut inbox, later);
        assert_eq!(order(&inbox), [(1, 4)]);
        // the second one is removed from the desk
        chat.forget(1);
        sync(&chat, &mut inbox, later);
        assert!(inbox.is_empty());
    }
}
