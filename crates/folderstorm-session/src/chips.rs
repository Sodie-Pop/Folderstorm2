//! Checkpoint 9 of the session host: a character's conversations as
//! **chips**, in the shared chat and in a monitor's chat (session-host
//! plan, checkpoint 9).
//!
//! The rules, all pure:
//! - **order** ([`order`]): the nearby chat, then the pinned
//!   conversations, then the unread ones, then the most recent;
//! - **names shrink to fit** ([`truncate`], [`layout`]): every chip is cut
//!   to the same length, the longest with which they all fit, never under
//!   [`MIN_CHARS`] characters and an ellipsis;
//! - **at most [`MAX_ROWS`] rows** ([`layout`]): what does not fit at the
//!   least length goes behind a "+N" chip at the end of the last row,
//!   which says how many of them have something unread. A new message
//!   makes its conversation unread, which brings it out by the order. The
//!   conversation on screen is never hidden.
//!
//! Nothing here measures a text: the caller says how wide a chip is for a
//! given length ([`Widths`]), so the rules are tested without a font.

/// The least a name is cut to, in characters, before its ellipsis.
pub const MIN_CHARS: usize = 6;
/// The most rows the chips take.
pub const MAX_ROWS: usize = 2;
/// "No cut": every name whole.
pub const WHOLE: usize = usize::MAX;

/// What the order and the layout need to know of one conversation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// The nearby chat: always first, never closed.
    pub nearby: bool,
    /// Pinned by the user: before the others.
    pub pinned: bool,
    /// Lines of others not seen yet.
    pub unread: u32,
    /// Grows with every batch of lines the conversation gets.
    pub recency: u64,
    /// The conversation on screen: never hidden.
    pub selected: bool,
    /// How long its name is, in characters.
    pub chars: usize,
}

/// `name` cut to `max` characters and an ellipsis. A name is left whole
/// when the cut would save nothing (it is at most one character longer
/// than `max`: the ellipsis takes that place). Never under [`MIN_CHARS`].
pub fn truncate(name: &str, max: usize) -> std::borrow::Cow<'_, str> {
    let max = max.max(MIN_CHARS);
    // (the byte where character `max + 1` starts: None when there is none)
    match name.char_indices().nth(max.saturating_add(1)) {
        None => std::borrow::Cow::Borrowed(name),
        Some(_) => {
            let end = name.char_indices().nth(max).map_or(name.len(), |(at, _)| at);
            let mut cut = name[..end].trim_end().to_owned();
            cut.push('…');
            std::borrow::Cow::Owned(cut)
        }
    }
}

/// Whether [`truncate`] cuts a name of `chars` characters at `max`.
pub fn is_cut(chars: usize, max: usize) -> bool {
    chars > max.max(MIN_CHARS).saturating_add(1)
}

/// The conversations' places among the chips: the nearby chat, the
/// pinned, the unread, then the most recent first. Equal ones keep the
/// order they were given in.
pub fn order(items: &[Item]) -> Vec<usize> {
    let mut indexes: Vec<usize> = (0..items.len()).collect();
    let group = |item: &Item| match (item.nearby, item.pinned, item.unread > 0) {
        (true, _, _) => 0,
        (false, true, _) => 1,
        (false, false, true) => 2,
        (false, false, false) => 3,
    };
    // (stable: equal keys stay as given)
    indexes.sort_by_key(|i| (group(&items[*i]), std::cmp::Reverse(items[*i].recency)));
    indexes
}

/// How wide things are, in the caller's units.
pub struct Widths<'a> {
    /// The room a row has.
    pub room: f32,
    /// The room between two chips of a row.
    pub gap: f32,
    /// The width of the chip of item `index` with its name cut to that
    /// many characters ([`WHOLE`]: not cut). Never wider for a shorter
    /// cut.
    pub chip: &'a dyn Fn(usize, usize) -> f32,
    /// The width of the "+N" chip for that many hidden conversations, of
    /// which that many have something unread.
    pub more: &'a dyn Fn(usize, usize) -> f32,
}

/// Where the chips go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// The rows, each the indexes of its chips from the left.
    pub rows: Vec<Vec<usize>>,
    /// What every name is cut to ([`WHOLE`]: none is cut).
    pub chars: usize,
    /// The conversations behind "+N", in their order; "+N" is drawn at
    /// the end of the last row when there are any.
    pub hidden: Vec<usize>,
    /// How many of the hidden ones have something unread.
    pub hidden_unread: usize,
}

impl Layout {
    /// The chips on screen, in their order.
    pub fn shown(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().flatten().copied()
    }
}

/// The rows `chips` take at that cut, with `tail` (the "+N" chip's
/// width, if there is one) at the end: each row as many as fit, one at
/// least.
fn pack(chips: &[usize], chars: usize, tail: Option<f32>, widths: &Widths<'_>) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0f32;
    let mut place = |width: f32, index: Option<usize>| match rows.last_mut() {
        Some(row) if used + widths.gap + width <= widths.room + 0.5 => {
            used += widths.gap + width;
            row.extend(index);
        }
        _ => {
            used = width;
            rows.push(index.into_iter().collect());
        }
    };
    for index in chips {
        place((widths.chip)(*index, chars), Some(*index));
    }
    if let Some(tail) = tail {
        place(tail, None);
    }
    rows
}

/// The longest cut, from [`MIN_CHARS`] to `longest` characters, at which
/// `fits`; None when not even the least does. (`fits` holds for every
/// cut shorter than one it holds for.)
fn longest_cut(longest: usize, fits: &dyn Fn(usize) -> bool) -> Option<usize> {
    if !fits(MIN_CHARS) {
        return None;
    }
    let (mut low, mut high) = (MIN_CHARS, longest.max(MIN_CHARS));
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if fits(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Some(low)
}

/// Lays the chips out: whole when they fit [`MAX_ROWS`] rows, else cut to
/// the longest length with which they do, else, at the least length, as
/// many as fit with a "+N" chip for the rest. The conversation on screen
/// is among those shown.
pub fn layout(items: &[Item], widths: &Widths<'_>) -> Layout {
    let ordered = order(items);
    if ordered.is_empty() {
        return Layout {
            chars: WHOLE,
            ..Layout::default()
        };
    }
    // (a chip wider than a row is no fit either: its name is cut first)
    let fits = |chips: &[usize], chars: usize, tail: Option<f32>| {
        pack(chips, chars, tail, widths).len() <= MAX_ROWS && chips.iter().all(|i| (widths.chip)(*i, chars) <= widths.room + 0.5)
    };
    if fits(&ordered, WHOLE, None) {
        return Layout {
            rows: pack(&ordered, WHOLE, None, widths),
            chars: WHOLE,
            ..Layout::default()
        };
    }
    let longest = items.iter().map(|item| item.chars).max().unwrap_or(0);
    if let Some(chars) = longest_cut(longest, &|chars| fits(&ordered, chars, None)) {
        return Layout {
            rows: pack(&ordered, chars, None, widths),
            chars,
            ..Layout::default()
        };
    }
    // not all of them, even at the least length: the first ones, the one
    // on screen among them, and "+N" for the rest
    let selected = ordered.iter().copied().find(|i| items[*i].selected);
    let split = |count: usize| {
        let mut shown: Vec<usize> = ordered[..count].to_vec();
        if let Some(selected) = selected.filter(|selected| !shown.contains(selected))
            && let Some(last) = shown.last_mut()
        {
            // (it takes the last place on screen)
            *last = selected;
        }
        let hidden: Vec<usize> = ordered.iter().copied().filter(|i| !shown.contains(i)).collect();
        (shown, hidden)
    };
    let tail = |hidden: &[usize]| {
        let unread = hidden.iter().filter(|i| items[**i].unread > 0).count();
        ((widths.more)(hidden.len(), unread), unread)
    };
    // (one chip at least, whatever the room: the window clips it)
    let count = (1..ordered.len())
        .rev()
        .find(|count| {
            let (shown, hidden) = split(*count);
            fits(&shown, MIN_CHARS, Some(tail(&hidden).0))
        })
        .unwrap_or(1);
    let (shown, hidden) = split(count);
    let (more, hidden_unread) = tail(&hidden);
    // fewer chips: their names may be longer again
    let chars = longest_cut(longest, &|chars| fits(&shown, chars, Some(more))).unwrap_or(MIN_CHARS);
    Layout {
        rows: pack(&shown, chars, Some(more), widths),
        chars,
        hidden,
        hidden_unread,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(chars: usize, recency: u64) -> Item {
        Item {
            chars,
            recency,
            ..Item::default()
        }
    }

    /// A font of 7 units a character: a chip is 30 units and its name;
    /// "+N" is 40.
    fn widths(room: f32, items: &[Item]) -> (f32, Vec<usize>) {
        (room, items.iter().map(|item| item.chars).collect())
    }

    fn lay(items: &[Item], room: f32) -> Layout {
        let (room, chars) = widths(room, items);
        let chip = move |index: usize, cut: usize| {
            let shown = if is_cut(chars[index], cut) {
                cut.max(MIN_CHARS) + 1
            } else {
                chars[index]
            };
            30.0 + 7.0 * shown as f32
        };
        layout(
            items,
            &Widths {
                room,
                gap: 6.0,
                chip: &chip,
                more: &|_, _| 40.0,
            },
        )
    }

    #[test]
    fn a_name_is_cut_with_an_ellipsis_and_never_under_the_least() {
        assert_eq!(truncate("Jane Doe", WHOLE), "Jane Doe");
        assert_eq!(truncate("Jane Doe", 8), "Jane Doe");
        // the ellipsis would take the place of one character: left whole
        assert_eq!(truncate("Jane Doe", 7), "Jane Doe");
        assert_eq!(truncate("Jane Doe", 6), "Jane D…");
        assert_eq!(truncate("Jane Doe", 2), "Jane D…", "never under the least");
        assert_eq!(truncate("Builders of the North", 8), "Builders…");
        // no space is left before the ellipsis
        assert_eq!(truncate("Builders of the North", 9), "Builders…");
        // characters, not bytes
        assert_eq!(truncate("日本語のグループ名前", 6), "日本語のグル…");
        assert_eq!(truncate("Zoë Müller-Lüdenscheidt", 7), "Zoë Mül…");
        assert_eq!(truncate("", 6), "");
        assert!(is_cut(9, 6) && !is_cut(7, 6) && !is_cut(40, WHOLE) && is_cut(8, 0));
        for (name, max) in [("Jane Doe", 6), ("Jane Doe", 7), ("Builders of the North", 12), ("ab", 6)] {
            assert_eq!(is_cut(name.chars().count(), max), truncate(name, max) != name, "{name} at {max}");
        }
    }

    #[test]
    fn nearby_comes_first_then_pinned_then_unread_then_recent() {
        let items = [
            item(8, 5),
            Item { unread: 2, ..item(8, 1) },
            Item {
                nearby: true,
                ..item(6, 0)
            },
            Item {
                pinned: true,
                ..item(8, 2)
            },
            item(8, 9),
            Item { unread: 1, ..item(8, 7) },
            Item {
                pinned: true,
                unread: 4,
                ..item(8, 3)
            },
        ];
        // nearby; the pinned by recency (unread or not); the unread by
        // recency; the rest by recency
        assert_eq!(order(&items), [2, 6, 3, 5, 1, 4, 0]);
        // equal ones stay as given
        let same = [item(4, 3), item(4, 3), item(4, 3)];
        assert_eq!(order(&same), [0, 1, 2]);
        assert!(order(&[]).is_empty());
    }

    #[test]
    fn a_new_message_brings_a_conversation_forward() {
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        items.extend((1..=6).map(|n| item(8, n)));
        // the oldest conversation is last
        assert_eq!(order(&items).last(), Some(&1));
        // it gets a message: unread, and the most recent
        items[1].unread = 1;
        items[1].recency = 10;
        assert_eq!(order(&items)[..2], [0, 1]);
    }

    #[test]
    fn chips_that_fit_are_whole_on_one_or_two_rows() {
        let items = [
            Item {
                nearby: true,
                ..item(6, 0)
            },
            item(8, 2),
            item(12, 1),
        ];
        // 72 + 6 + 86 + 6 + 114 = 284
        let one = lay(&items, 300.0);
        assert_eq!(one.rows, [vec![0, 1, 2]]);
        assert_eq!(one.chars, WHOLE);
        assert!(one.hidden.is_empty());
        // narrower: two rows, still whole
        let two = lay(&items, 200.0);
        assert_eq!(two.rows, [vec![0, 1], vec![2]]);
        assert_eq!(two.chars, WHOLE);
        assert_eq!(lay(&[], 200.0).rows, Vec::<Vec<usize>>::new());
    }

    #[test]
    fn names_shrink_together_before_anything_is_hidden() {
        let items = [
            Item {
                nearby: true,
                ..item(6, 0)
            },
            item(20, 4),
            item(24, 3),
            item(18, 2),
            item(9, 1),
        ];
        // whole: 72, 170, 198, 156, 93: three rows of 320
        let cut = lay(&items, 320.0);
        assert!(cut.hidden.is_empty(), "{cut:?}");
        assert!(cut.rows.len() <= MAX_ROWS);
        assert!(cut.chars >= MIN_CHARS && cut.chars < 24, "{cut:?}");
        assert_eq!(cut.shown().collect::<Vec<_>>(), order(&items));
        // the longest cut that fits: one character more does not
        let (room, chars) = widths(320.0, &items);
        let chip = move |index: usize, cut: usize| {
            let shown = if is_cut(chars[index], cut) { cut + 1 } else { chars[index] };
            30.0 + 7.0 * shown as f32
        };
        let w = Widths {
            room,
            gap: 6.0,
            chip: &chip,
            more: &|_, _| 40.0,
        };
        assert!(pack(&order(&items), cut.chars + 1, None, &w).len() > MAX_ROWS);
        assert_eq!(pack(&order(&items), cut.chars, None, &w), cut.rows);
    }

    #[test]
    fn what_does_not_fit_two_rows_goes_behind_plus_n() {
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        items.extend((1..=12).map(|n| item(14, n)));
        items[3].unread = 2;
        items[5].unread = 1;
        // at the least length a chip is 30 + 7 × 7 = 79: three a row of
        // 260, so five chips and "+N" on two rows
        let l = lay(&items, 260.0);
        assert_eq!(l.rows.len(), MAX_ROWS);
        assert_eq!(l.shown().count(), 5);
        // (five chips and "+N" leave room for one more character)
        assert_eq!(l.chars, MIN_CHARS + 1);
        assert_eq!(l.hidden.len(), 8);
        // every conversation is somewhere, once
        let mut all: Vec<usize> = l.shown().chain(l.hidden.iter().copied()).collect();
        all.sort_unstable();
        assert_eq!(all, (0..items.len()).collect::<Vec<_>>());
        // the nearby chat and the unread ones are on screen
        let shown: Vec<usize> = l.shown().collect();
        assert_eq!(shown[..3], [0, 5, 3]);
        assert_eq!(l.hidden_unread, 0);
        // the hidden ones keep the order: the most recent first
        assert_eq!(l.hidden[..2], [10, 9]);
        // an unread one behind "+N" is counted
        let mut many = items.clone();
        for item in &mut many[1..] {
            item.unread = 1;
        }
        let l = lay(&many, 260.0);
        assert_eq!(l.hidden_unread, l.hidden.len());
    }

    #[test]
    fn a_new_message_comes_out_of_plus_n() {
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        items.extend((1..=12).map(|n| item(14, n)));
        let before = lay(&items, 260.0);
        assert!(before.hidden.contains(&1), "the oldest is hidden: {before:?}");
        items[1].unread = 1;
        items[1].recency = 20;
        let after = lay(&items, 260.0);
        assert!(after.shown().any(|i| i == 1), "{after:?}");
        assert_eq!(after.shown().count(), before.shown().count());
    }

    #[test]
    fn the_conversation_on_screen_is_never_hidden() {
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        items.extend((1..=12).map(|n| item(14, n)));
        items[1].selected = true;
        let l = lay(&items, 260.0);
        let shown: Vec<usize> = l.shown().collect();
        assert_eq!(shown.last(), Some(&1), "it takes the last place: {l:?}");
        assert!(!l.hidden.contains(&1));
        assert_eq!(shown.len() + l.hidden.len(), items.len());
        // one that is on screen anyway stays where the order puts it
        items[1].selected = false;
        items[12].selected = true;
        let l = lay(&items, 260.0);
        assert_eq!(l.shown().nth(1), Some(12));
    }

    #[test]
    fn fewer_chips_get_longer_names_again() {
        // the nearby chat, two long names, two short ones, three long
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        for (n, chars) in [30, 30, 4, 4, 30, 30, 30].into_iter().enumerate() {
            items.push(item(chars, 10 - n as u64));
        }
        // at the least length: 72, 79, 79 | 58, 58, 79, 79 and one more
        // row: "+N" takes the last two, and the names the room they left
        let l = lay(&items, 300.0);
        assert_eq!(l.rows, [vec![0, 1, 2], vec![3, 4, 5]]);
        assert_eq!(l.hidden, [6, 7]);
        assert_eq!(l.chars, 10, "{l:?}");
        // the longest that fits: one character more is a third row
        let (room, chars) = widths(300.0, &items);
        let chip = move |index: usize, cut: usize| {
            let shown = if is_cut(chars[index], cut) { cut + 1 } else { chars[index] };
            30.0 + 7.0 * shown as f32
        };
        let w = Widths {
            room,
            gap: 6.0,
            chip: &chip,
            more: &|_, _| 40.0,
        };
        let shown: Vec<usize> = l.shown().collect();
        assert!(pack(&shown, l.chars + 1, Some(40.0), &w).len() > MAX_ROWS);
    }

    #[test]
    fn a_name_wider_than_the_row_is_cut_although_two_rows_would_hold_it() {
        // the nearby chat and one long name: two rows either way
        let items = [
            Item {
                nearby: true,
                ..item(6, 0)
            },
            item(40, 1),
        ];
        let l = lay(&items, 200.0);
        assert_eq!(l.rows, [vec![0], vec![1]]);
        assert!(l.hidden.is_empty());
        // 30 + 7 × (cut + 1) ≤ 200
        assert_eq!(l.chars, 23);
    }

    #[test]
    fn a_window_too_narrow_for_anything_still_shows_one_chip() {
        let mut items = vec![Item {
            nearby: true,
            ..item(6, 0)
        }];
        items.extend((1..=4).map(|n| item(14, n)));
        let l = lay(&items, 20.0);
        assert_eq!(l.shown().collect::<Vec<_>>(), [0]);
        assert_eq!(l.hidden.len(), 4);
        assert_eq!(l.chars, MIN_CHARS);
        // and the one on screen, when it is not the nearby chat
        items[3].selected = true;
        let l = lay(&items, 20.0);
        assert_eq!(l.shown().collect::<Vec<_>>(), [3]);
    }
}
