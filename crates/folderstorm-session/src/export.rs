//! A managed viewer tells the host its chat by **observation**
//! (session-host plan, section 9): nothing in the viewer's chat code calls
//! the session host. Now and then the viewer's adapter looks at each list
//! of lines and at the clock the viewer keeps for it (`LineClock` in
//! `world/transcripts.rs`: a number for every line that stays the same
//! while lines are added at either end or dropped from the front, a
//! revision that changes whenever the lines do, a token that changes when
//! the numbers no longer name the same lines), and a [`Cursor`] says which
//! lines are new.
//!
//! - Only lines after the last one told are told; history put before the
//!   first line later is not (transcripts stay in the viewer).
//! - A token change, or [`Cursor::restart`] (what the viewer shows of old
//!   lines changed: a restriction now hides a name), starts again from the
//!   last [`BACKLOG`] lines, and the host replaces what it held.
//! - Lines dropped from the front before they were told are counted: the
//!   host marks a gap, never a silent hole.
//! - The cursor moves only over what the link accepted
//!   ([`Cursor::told`]): a full queue delays lines, it never loses them.
//!
//! [`Sends`] is the viewer's other rule: a line the host asks it to say is
//! said at most once.

use std::ops::Range;

/// How many of a conversation's last lines are told when its export
/// starts or starts again.
pub const BACKLOG: usize = 50;

/// A list of lines as its viewer numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Number of the first line; line `i` has number `first + i`.
    pub first: u64,
    pub revision: u64,
    pub token: u64,
    /// How many lines the list holds.
    pub len: usize,
}

impl Clock {
    /// The number after the last line's.
    fn end(&self) -> u64 {
        self.first + self.len as u64
    }
}

/// What to tell of a list now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The host replaces what it holds of this conversation.
    pub restart: bool,
    /// Lines dropped before they were told, just before `lines`.
    pub missed: u64,
    /// The lines to tell, by their place in the list.
    pub lines: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    token: u64,
    /// The revision at which everything was told (None: lines wait).
    revision: Option<u64>,
    /// The number of the next line to tell.
    next: u64,
}

/// Where the export of one list of lines is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    seen: Option<Seen>,
    /// The next plan starts again, even on a list that did not change.
    again: bool,
}

impl Cursor {
    /// What to tell of the list; None when nothing is new. The plan is
    /// carried out, wholly or in part, then [`Cursor::told`] says how far.
    pub fn plan(&mut self, clock: Clock) -> Option<Plan> {
        let Some(seen) = self.seen.filter(|s| s.token == clock.token && !self.again) else {
            let known = self.seen.is_some();
            if clock.len == 0 && !known {
                // nothing told, nothing to tell: wait for a first line
                return None;
            }
            return Some(Plan {
                restart: true,
                missed: 0,
                lines: clock.len.saturating_sub(BACKLOG)..clock.len,
            });
        };
        if seen.revision == Some(clock.revision) {
            return None;
        }
        if seen.next >= clock.end() {
            // history before the first line, or lines dropped that were told
            self.seen = Some(Seen {
                revision: Some(clock.revision),
                ..seen
            });
            return None;
        }
        let missed = clock.first.saturating_sub(seen.next);
        let start = seen.next.saturating_sub(clock.first) as usize;
        Some(Plan {
            restart: false,
            missed,
            lines: start..clock.len,
        })
    }

    /// The link took the plan's lines up to place `end` (not included) of
    /// the list as `clock` showed it. With `end` short of the list's end,
    /// the rest follows in the next plan.
    pub fn told(&mut self, clock: Clock, end: usize) {
        let end = end.min(clock.len);
        self.again = false;
        self.seen = Some(Seen {
            token: clock.token,
            revision: (end == clock.len).then_some(clock.revision),
            next: clock.first + end as u64,
        });
    }

    /// Something of the list was told.
    pub fn started(&self) -> bool {
        self.seen.is_some()
    }

    /// What the viewer shows of lines already told changed: the next plan
    /// tells the conversation again.
    pub fn restart(&mut self) {
        if self.seen.is_some() {
            self.again = true;
        }
    }
}

/// The lines the host asked this viewer to say, by their numbers: each is
/// said at most once, whatever the pipe repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sends {
    last: u64,
}

impl Sends {
    /// Whether line `id` is new: above every one seen. A repeated or older
    /// number is never said again.
    pub fn fresh(&mut self, id: u64) -> bool {
        if id <= self.last {
            return false;
        }
        self.last = id;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list like the viewer's: lines by their numbers, and its clock.
    struct List {
        clock: Clock,
        tick: u64,
    }

    impl List {
        fn new() -> Self {
            Self {
                clock: Clock {
                    first: 1 << 40,
                    revision: 1,
                    token: 2,
                    len: 0,
                },
                tick: 2,
            }
        }
        fn tick(&mut self) -> u64 {
            self.tick += 1;
            self.tick
        }
        fn push(&mut self, count: usize) {
            self.clock.len += count;
            self.clock.revision = self.tick();
        }
        fn pop(&mut self, count: usize) {
            self.clock.len -= count;
            self.clock.first += count as u64;
            self.clock.revision = self.tick();
        }
        fn history(&mut self, count: usize) {
            self.clock.len += count;
            self.clock.first -= count as u64;
            self.clock.revision = self.tick();
        }
        fn renumber(&mut self, len: usize) {
            self.clock.len = len;
            self.clock.revision = self.tick();
            self.clock.token = self.tick();
        }
    }

    /// Carries a plan out whole; gives the numbers of the lines told.
    fn tell(cursor: &mut Cursor, list: &List) -> Option<(bool, u64, Vec<u64>)> {
        let plan = cursor.plan(list.clock)?;
        let numbers = plan.lines.clone().map(|i| list.clock.first + i as u64).collect();
        cursor.told(list.clock, plan.lines.end);
        Some((plan.restart, plan.missed, numbers))
    }

    const BASE: u64 = 1 << 40;

    #[test]
    fn only_new_lines_are_told_each_once() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        assert_eq!(tell(&mut cursor, &list), None, "an empty list tells nothing");
        list.push(2);
        assert_eq!(tell(&mut cursor, &list), Some((true, 0, vec![BASE, BASE + 1])));
        assert_eq!(tell(&mut cursor, &list), None);
        list.push(1);
        assert_eq!(tell(&mut cursor, &list), Some((false, 0, vec![BASE + 2])));
        list.push(3);
        assert_eq!(tell(&mut cursor, &list), Some((false, 0, vec![BASE + 3, BASE + 4, BASE + 5])));
        assert_eq!(tell(&mut cursor, &list), None);
    }

    #[test]
    fn a_first_look_tells_the_last_lines_only() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        list.push(BACKLOG + 30);
        let (restart, missed, numbers) = tell(&mut cursor, &list).expect("lines");
        assert!(restart);
        assert_eq!(missed, 0);
        assert_eq!(numbers.len(), BACKLOG);
        assert_eq!(numbers.first(), Some(&(BASE + 30)));
    }

    #[test]
    fn history_and_dropped_old_lines_tell_nothing() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        list.push(3);
        tell(&mut cursor, &list);
        // the transcript's end is put before the first line
        list.history(20);
        assert_eq!(tell(&mut cursor, &list), None);
        // old lines leave by the front
        list.pop(5);
        assert_eq!(tell(&mut cursor, &list), None);
        list.push(1);
        assert_eq!(tell(&mut cursor, &list), Some((false, 0, vec![BASE + 3])));
    }

    #[test]
    fn a_token_change_starts_again_and_the_host_replaces_what_it_held() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        list.push(4);
        tell(&mut cursor, &list);
        // the nearby chat is cleared: an empty restart empties the host's copy
        list.renumber(0);
        assert_eq!(tell(&mut cursor, &list), Some((true, 0, vec![])));
        assert_eq!(tell(&mut cursor, &list), None);
        list.push(1);
        assert_eq!(tell(&mut cursor, &list), Some((false, 0, vec![BASE])));
        // lines inserted in the middle: told again from the last ones
        list.renumber(6);
        let (restart, _, numbers) = tell(&mut cursor, &list).expect("lines");
        assert!(restart);
        assert_eq!(numbers.len(), 6);
    }

    #[test]
    fn lines_dropped_before_they_were_told_are_counted() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        list.push(2);
        tell(&mut cursor, &list);
        // ten more arrive and the first seven of the list leave untold
        list.push(10);
        list.pop(7);
        let (restart, missed, numbers) = tell(&mut cursor, &list).expect("lines");
        assert!(!restart);
        assert_eq!(missed, 5, "numbers 2 to 6 were never told");
        assert_eq!(numbers, (BASE + 7..BASE + 12).collect::<Vec<_>>());
    }

    #[test]
    fn a_full_queue_delays_lines_and_never_loses_them() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        list.push(5);
        // the link takes two lines of the five
        let plan = cursor.plan(list.clock).expect("plan");
        assert_eq!(plan.lines, 0..5);
        cursor.told(list.clock, 2);
        // the rest follows, without a restart, even though nothing changed
        assert_eq!(tell(&mut cursor, &list), Some((false, 0, vec![BASE + 2, BASE + 3, BASE + 4])));
        assert_eq!(tell(&mut cursor, &list), None);
        // the link takes nothing at all: the plan comes back as it was
        list.push(1);
        let plan = cursor.plan(list.clock).expect("plan");
        assert_eq!(cursor.plan(list.clock), Some(plan));
    }

    #[test]
    fn a_restart_tells_the_conversation_again_once() {
        let (mut list, mut cursor) = (List::new(), Cursor::default());
        // (nothing told yet: nothing to tell again)
        cursor.restart();
        assert_eq!(tell(&mut cursor, &list), None);
        list.push(3);
        tell(&mut cursor, &list);
        cursor.restart();
        assert_eq!(tell(&mut cursor, &list), Some((true, 0, vec![BASE, BASE + 1, BASE + 2])));
        assert_eq!(tell(&mut cursor, &list), None);
    }

    #[test]
    fn a_line_to_say_is_said_at_most_once() {
        let mut sends = Sends::default();
        assert!(sends.fresh(1));
        assert!(!sends.fresh(1), "the same number again");
        assert!(sends.fresh(2));
        assert!(sends.fresh(5));
        assert!(!sends.fresh(4), "an older number that comes late");
        assert!(!sends.fresh(0));
    }
}
