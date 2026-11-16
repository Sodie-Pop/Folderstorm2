//! Which layout a quick switch means (folderstorm/plans/workspaces.md,
//! checkpoint 5 in detail, point 2). Pure: the switcher's list, its filter
//! and its selection, the toolbar's toggle, the favorites' round.
//!
//! Folderstorm (C++): "type part of a name and press Enter to switch.
//! Favorites appear first" (doc/help.md "Workspaces"), and next / previous
//! favorite from the keyboard (doc/plans/workspace-enhancements.md, 3).

use super::store::{Favorite, Store, same_name};

/// What a line names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Layout,
    Workspace,
}

/// A line of the switcher: a layout or a workspace that can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// Where it is in the store's list of its kind.
    pub index: usize,
    pub favorite: bool,
}

impl Entry {
    /// Its name.
    pub fn name<'a>(&self, store: &'a Store) -> Option<&'a str> {
        match self.kind {
            Kind::Layout => store.layout(self.index).map(|layout| layout.name.as_str()),
            Kind::Workspace => store.workspace(self.index).map(|workspace| workspace.name.as_str()),
        }
    }

    /// Whether it is the layout, or the workspace, in use.
    pub fn in_use(&self, store: &Store) -> bool {
        match self.kind {
            Kind::Layout => store.is_active(self.index),
            Kind::Workspace => store.is_active_workspace(self.index),
        }
    }

    /// Whether it is the entry of this kind and name.
    pub fn is(&self, store: &Store, kind: Kind, name: &str) -> bool {
        self.kind == kind && self.name(store).is_some_and(|own| same_name(own, name))
    }
}

/// The switcher's lines: the usable favorites in their order (layouts and
/// workspaces), then the other usable workspaces, then the other usable
/// layouts, each in its list's order. Each entry once.
pub fn entries(store: &Store) -> Vec<Entry> {
    let mut lines: Vec<Entry> = store
        .favorites
        .iter()
        .filter_map(|favorite| match favorite {
            Favorite::Layout(name) => store.find(name).map(|(index, _)| (Kind::Layout, index)),
            Favorite::Workspace(name) => store.find_workspace(name).map(|(index, _)| (Kind::Workspace, index)),
            Favorite::Kept(_) => None,
        })
        .map(|(kind, index)| Entry {
            kind,
            index,
            favorite: true,
        })
        .collect();
    let favorites = lines.len();
    let others = (0..store.workspaces.len())
        .filter(|index| store.workspace(*index).is_some())
        .map(|index| (Kind::Workspace, index))
        .chain(
            (0..store.layouts.len())
                .filter(|index| store.layout(*index).is_some())
                .map(|index| (Kind::Layout, index)),
        );
    for (kind, index) in others {
        if !lines[..favorites].iter().any(|line| line.kind == kind && line.index == index) {
            lines.push(Entry {
                kind,
                index,
                favorite: false,
            });
        }
    }
    lines
}

/// Whether `name` answers what was typed: every word of `typed` is found
/// in it, ignoring case. Nothing typed answers every name.
pub fn matches(name: &str, typed: &str) -> bool {
    let name = name.to_lowercase();
    typed.split_whitespace().all(|word| name.contains(&word.to_lowercase()))
}

/// The lines left by what was typed, in the same order: favorites stay
/// first.
pub fn filtered(store: &Store, typed: &str) -> Vec<Entry> {
    entries(store)
        .into_iter()
        .filter(|line| line.name(store).is_some_and(|name| matches(name, typed)))
        .collect()
}

/// The line selected before the user chooses one, as a place in `lines`:
/// with nothing typed, the first entry that is not in use (open the
/// switcher, press Enter: the other one); once something is typed, the
/// first that answers it.
pub fn first_choice(store: &Store, lines: &[Entry], typed: &str) -> Option<usize> {
    if typed.trim().is_empty()
        && let Some(other) = lines.iter().position(|line| !line.in_use(store))
    {
        return Some(other);
    }
    (!lines.is_empty()).then_some(0)
}

/// The place after `at` among `count` lines, going `down` or up, round the
/// ends. From no selection: the first going down, the last going up.
pub fn step(count: usize, at: Option<usize>, down: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (at.filter(|at| *at < count), down) {
        (Some(at), true) => (at + 1) % count,
        (Some(at), false) => (at + count - 1) % count,
        (None, true) => 0,
        (None, false) => count - 1,
    })
}

/// Why the toolbar's toggle has nothing to switch to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoToggle {
    /// One of its two layouts is not chosen, or is no longer usable.
    NotChosen,
    /// Both places hold the same layout.
    Same,
}

/// The layout the toggle would apply now, as a place in the store's list:
/// the second of its two when the first is in use, the first otherwise.
pub fn toggle_target(store: &Store) -> Result<usize, NoToggle> {
    let chosen = |place: usize| {
        store.toggle[place]
            .as_deref()
            .and_then(|name| store.find(name))
            .map(|(index, _)| index)
            .ok_or(NoToggle::NotChosen)
    };
    let (first, second) = (chosen(0)?, chosen(1)?);
    if first == second {
        return Err(NoToggle::Same);
    }
    Ok(if store.is_active(first) { second } else { first })
}

/// The favorite after (`forward`) or before the layout in use, round the
/// ends, as a place in the store's list. When the layout in use is not a
/// favorite (or none is in use): the first going forward, the last going
/// back. None without a usable favorite.
pub fn next_favorite(store: &Store, forward: bool) -> Option<usize> {
    let favorites: Vec<usize> = store.favorite_layouts().map(|(index, _)| index).collect();
    let active = store.active_layout.as_deref();
    let at = favorites.iter().position(|index| {
        store
            .layout(*index)
            .zip(active)
            .is_some_and(|(layout, active)| same_name(&layout.name, active))
    });
    step(favorites.len(), at, forward).map(|place| favorites[place])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::store::Layout;
    use serde_json::json;

    fn layout(name: &str) -> Layout {
        Layout::new(name, Some(Default::default()), None, Vec::new())
    }

    /// "2 monitors", "1 monitor", "Photos", "Sorting", none in use.
    fn store() -> Store {
        let mut store = Store::default();
        for name in ["2 monitors", "1 monitor", "Photos", "Sorting"] {
            store.add(layout(name)).expect("added");
        }
        store.set_active(None);
        store
    }

    fn names(store: &Store, lines: &[Entry]) -> Vec<String> {
        lines.iter().map(|line| line.name(store).expect("usable").to_owned()).collect()
    }

    #[test]
    fn workspaces_come_after_the_favorites_and_before_the_layouts() {
        use crate::folderstorm::workspaces::workspace::Workspace;
        let mut store = store();
        for name in ["Sorting", "Exploring"] {
            store
                .add_workspace(Workspace::new(name, None, std::collections::BTreeSet::new()))
                .expect("added");
        }
        let lines = entries(&store);
        assert_eq!(
            names(&store, &lines),
            ["Sorting", "Exploring", "2 monitors", "1 monitor", "Photos", "Sorting"]
        );
        assert_eq!(lines[0].kind, Kind::Workspace);
        assert_eq!(lines[5].kind, Kind::Layout, "a layout of the same name is another entry");
        store.set_favorite("1 monitor", true);
        store.set_favorite_workspace("Exploring", true);
        let lines = entries(&store);
        assert_eq!(
            names(&store, &lines),
            ["1 monitor", "Exploring", "Sorting", "2 monitors", "Photos", "Sorting"]
        );
        assert_eq!(lines.iter().filter(|line| line.favorite).count(), 2);
        assert!(lines[1].is(&store, Kind::Workspace, "exploring") && !lines[1].is(&store, Kind::Layout, "Exploring"));
        // the filter and the first choice go through both kinds
        assert_eq!(names(&store, &filtered(&store, "sort")), ["Sorting", "Sorting"]);
        assert!(lines[1].in_use(&store), "the workspace added last is in use");
        store.set_active(Some("1 monitor".to_owned()));
        assert_eq!(first_choice(&store, &lines, ""), Some(2), "the first that is not in use");
    }

    #[test]
    fn favorites_come_first_in_their_order_and_each_layout_once() {
        let mut store = store();
        assert_eq!(names(&store, &entries(&store)), ["2 monitors", "1 monitor", "Photos", "Sorting"]);
        assert!(entries(&store).iter().all(|line| !line.favorite));
        store.set_favorite("Sorting", true);
        store.set_favorite("1 monitor", true);
        let lines = entries(&store);
        assert_eq!(names(&store, &lines), ["Sorting", "1 monitor", "2 monitors", "Photos"]);
        assert_eq!(
            lines.iter().map(|line| line.favorite).collect::<Vec<_>>(),
            [true, true, false, false]
        );
    }

    #[test]
    fn an_entry_that_cannot_be_used_is_not_offered() {
        let store: Store = serde_json::from_value(json!({
            "layouts": [layout("Wide"), { "schema": 7, "name": "Later" }, layout("Single")],
            "favorites": [{ "layout": "Later" }, { "layout": "Single" }],
        }))
        .expect("read");
        let lines = entries(&store);
        assert_eq!(names(&store, &lines), ["Single", "Wide"]);
        assert_eq!(
            lines[0],
            Entry {
                kind: Kind::Layout,
                index: 2,
                favorite: true
            }
        );
    }

    #[test]
    fn the_filter_finds_every_word_typed_whatever_the_case() {
        assert!(matches("2 monitors", ""));
        assert!(matches("2 monitors", "  "));
        assert!(matches("2 monitors", "MON"));
        assert!(matches("2 monitors", "mon 2"), "the words in any order");
        assert!(!matches("2 monitors", "mon 1"));
        assert!(matches("Écran de côté", "écran CÔTÉ"));
        assert!(matches("写真", "写"));
        assert!(!matches("Photos", "x"));
        let mut store = store();
        store.set_favorite("1 monitor", true);
        assert_eq!(names(&store, &filtered(&store, "mon")), ["1 monitor", "2 monitors"]);
        assert_eq!(
            names(&store, &filtered(&store, "o")),
            ["1 monitor", "2 monitors", "Photos", "Sorting"]
        );
        assert!(filtered(&store, "nothing like it").is_empty());
    }

    #[test]
    fn the_first_choice_is_the_other_layout_then_the_first_match() {
        let mut store = store();
        let lines = entries(&store);
        assert_eq!(first_choice(&store, &lines, ""), Some(0), "none in use: the first");
        store.set_active(Some("2 monitors".to_owned()));
        assert_eq!(first_choice(&store, &lines, ""), Some(1), "the first that is not in use");
        // once typed, the first that answers, in use or not
        let typed = filtered(&store, "mon");
        assert_eq!(first_choice(&store, &typed, "mon"), Some(0));
        assert_eq!(first_choice(&store, &[], "zzz"), None);
        // a single layout, in use: it is the choice all the same
        let mut one = Store::default();
        one.add(layout("Only")).expect("added");
        assert_eq!(first_choice(&one, &entries(&one), ""), Some(0));
        assert_eq!(first_choice(&Store::default(), &[], ""), None);
    }

    #[test]
    fn the_arrows_go_round_the_list() {
        assert_eq!(step(0, None, true), None);
        assert_eq!(step(3, None, true), Some(0));
        assert_eq!(step(3, None, false), Some(2));
        assert_eq!(step(3, Some(0), true), Some(1));
        assert_eq!(step(3, Some(2), true), Some(0));
        assert_eq!(step(3, Some(0), false), Some(2));
        assert_eq!(step(1, Some(0), true), Some(0));
        // a selection the filter left behind counts as none
        assert_eq!(step(2, Some(5), true), Some(0));
    }

    #[test]
    fn the_toggle_goes_to_the_one_of_its_two_that_is_not_in_use() {
        let mut store = store();
        assert_eq!(toggle_target(&store), Err(NoToggle::NotChosen));
        store.set_toggle(0, Some("2 monitors"));
        assert_eq!(toggle_target(&store), Err(NoToggle::NotChosen), "one of two");
        store.set_toggle(1, Some("2 monitors"));
        assert_eq!(toggle_target(&store), Err(NoToggle::Same));
        store.set_toggle(1, Some("1 monitor"));
        assert_eq!(toggle_target(&store), Ok(0), "neither in use: the first");
        store.set_active(Some("2 monitors".to_owned()));
        assert_eq!(toggle_target(&store), Ok(1));
        store.set_active(Some("1 monitor".to_owned()));
        assert_eq!(toggle_target(&store), Ok(0));
        store.set_active(Some("Photos".to_owned()));
        assert_eq!(toggle_target(&store), Ok(0), "another one in use: the first");
        // one of the two deleted: nothing to switch to
        store.remove(1);
        assert_eq!(toggle_target(&store), Err(NoToggle::NotChosen));
    }

    #[test]
    fn the_favorites_go_round_from_the_layout_in_use() {
        let mut store = store();
        assert_eq!(next_favorite(&store, true), None, "no favorite");
        for name in ["Sorting", "2 monitors", "Photos"] {
            store.set_favorite(name, true);
        }
        let name = |index: Option<usize>| index.and_then(|index| store.layout(index)).map(|layout| layout.name.clone());
        assert_eq!(
            name(next_favorite(&store, true)).as_deref(),
            Some("Sorting"),
            "none in use: the first"
        );
        assert_eq!(
            name(next_favorite(&store, false)).as_deref(),
            Some("Photos"),
            "none in use: the last"
        );
        let mut store = store.clone();
        let mut round = Vec::new();
        for _ in 0..4 {
            let next = next_favorite(&store, true).expect("a favorite");
            let name = store.layout(next).expect("usable").name.clone();
            store.set_active(Some(name.clone()));
            round.push(name);
        }
        assert_eq!(round, ["Sorting", "2 monitors", "Photos", "Sorting"]);
        let back = next_favorite(&store, false).expect("a favorite");
        assert_eq!(store.layout(back).expect("usable").name, "Photos");
        // in use, not a favorite: the ends
        store.set_active(Some("1 monitor".to_owned()));
        let first = next_favorite(&store, true).expect("a favorite");
        assert_eq!(store.layout(first).expect("usable").name, "Sorting");
        // a single favorite, in use: itself (applied again)
        let mut one = Store::default();
        one.add(layout("Only")).expect("added");
        one.set_favorite("Only", true);
        assert_eq!(next_favorite(&one, true), Some(0));
    }
}
