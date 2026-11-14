//! Frame tests of the favorites strip (FEATURES.md row 1.6): the island of
//! its own and the landmark row's right end, a click that applies, what
//! does not fit in *More…*, the setting. The same pretend viewer as the
//! others, with the top bar's two calls (`Bars`).

use super::*;
use crate::folderstorm::workspaces::menu::{self, Request};
use crate::folderstorm::workspaces::strip::{self, DIVIDER, ISLAND, NAME_CHARS, SHARE, fits};
use crate::ui::bars::favorites;
use crate::ui::bars::islands::{self, GAP, MARGIN, PAD, SLOT};
use aurora_i18n::tr;

/// The place island of a 1600 px wide viewer, more or less.
fn under(width: f32) -> Rect {
    Rect::from_min_size(pos2(60.0, 8.0), vec2(width, 36.0))
}

fn landmark(name: &str) -> favorites::Favorite {
    favorites::Favorite {
        item: uuid::Uuid::new_v4(),
        asset: uuid::Uuid::new_v4(),
        name: name.to_owned(),
        region: String::new(),
        rating: "",
        global: None,
        slurl: None,
        thumbnail: None,
    }
}

fn landmarks(count: usize, folded: bool) -> favorites::View {
    favorites::View {
        list: (0..count).map(|place| landmark(&format!("Landmark {place}"))).collect(),
        access: crate::settings::FavoritesAccess::Row,
        folded,
        ..Default::default()
    }
}

impl Viewer {
    fn island(&self) -> Option<Rect> {
        strip::own_island(&self.ctx)
    }

    /// The left edges of these texts, as drawn.
    fn lefts(&self, names: &[&str]) -> Vec<f32> {
        names.iter().map(|name| self.text_rect(name).left()).collect()
    }

    fn strip_view(&self) -> std::sync::Arc<menu::View> {
        menu::view(&self.ctx)
    }

    fn in_use(&self) -> Vec<String> {
        let view = self.strip_view();
        view.favorites.iter().filter(|f| f.in_use).map(|f| f.name.clone()).collect()
    }
}

/// [`two_layouts`] ("Wide", "Single" in use) and the workspace "Sorting",
/// the three favorites in the order Wide, Sorting, Single; the strip on,
/// under a top bar without a favourites' row.
fn with_strip() -> Viewer {
    let mut v = two_layouts();
    let ctx = v.ctx.clone();
    let keep = apply::Keep {
        layout: false,
        folders: false,
    };
    apply::save_new_workspace(&ctx, &mut v.live(), "Sorting", keep).expect("saved");
    assert!(v.store.set_favorite("Wide", true));
    assert!(v.store.set_favorite_workspace("Sorting", true));
    assert!(v.store.set_favorite("Single", true));
    v.store.strip = true;
    v.bars = Some(Bars {
        under: under(900.0),
        landmarks: None,
    });
    v.settle();
    v.saved = false;
    v
}

#[test]
fn more_is_always_kept_and_the_pills_that_fit_come_first() {
    let more = |left: usize| if left == 0 { 28.0 } else { 60.0 };
    let widths = [80.0, 80.0, 80.0];
    assert_eq!(fits(&widths, more, 1000.0), 3);
    // three pills and their gaps, then the small *More…*
    let all = 3.0 * (80.0 + GAP) + 28.0;
    assert_eq!(fits(&widths, more, all), 3);
    assert_eq!(fits(&widths, more, all - 1.0), 2, "the third goes to More…, which is wider for it");
    assert_eq!(fits(&widths, more, 2.0 * (80.0 + GAP) + 60.0 - 1.0), 1);
    assert_eq!(fits(&widths, more, 60.0), 0, "More… alone");
    assert_eq!(fits(&widths, more, 0.0), 0);
    assert_eq!(fits(&[], more, 0.0), 0);
}

#[test]
fn a_long_name_is_cut_on_its_pill() {
    assert_eq!(strip::short("Sorting"), "Sorting");
    let long = "A very long name of a layout";
    let cut = strip::short(long);
    assert_eq!(cut.chars().count(), NAME_CHARS);
    assert!(cut.ends_with('…') && long.starts_with(cut.trim_end_matches('…')));
}

#[test]
fn the_strip_is_off_until_chosen_and_the_file_says_nothing_of_it() {
    let store = Store::default();
    assert!(!store.strip);
    let saved = serde_json::to_value(&store).expect("saved");
    assert!(saved.get("strip").is_none(), "left out while off");
    let on = Store {
        strip: true,
        ..Default::default()
    };
    let saved = serde_json::to_value(&on).expect("saved");
    assert_eq!(saved["strip"], serde_json::json!(true));
    let read: Store = serde_json::from_value(saved).expect("read");
    assert!(read.strip);
    // anything else reads as off, and the rest of the store is read
    let odd: Store = serde_json::from_value(serde_json::json!({ "strip": "yes", "window_held": false })).expect("read");
    assert!(!odd.strip);
}

#[test]
fn the_strip_is_an_island_of_its_own_with_the_favorites_in_their_order() {
    let mut v = with_strip();
    let island = v.island().expect("drawn");
    let bars = under(900.0);
    assert_eq!(island.top(), bars.bottom() + MARGIN, "where the favourites' row would be");
    assert!((island.left() - bars.left()).abs() <= 1.0);
    assert!(island.width() < 500.0, "as wide as what it holds, not the whole row");
    let lefts = v.lefts(&["Wide", "Sorting", "Single"]);
    assert!(lefts[0] > island.left() && lefts[0] < lefts[1] && lefts[1] < lefts[2], "{lefts:?}");
    assert!(v.text_rect("Single").right() < island.right());
    // the windows stay under it, as under the favourites' row
    assert_eq!(islands::favorites_row_bottom(&v.ctx), Some(island.bottom()));
    // off: nothing of it
    v.store.strip = false;
    v.frames(3);
    assert!(v.island().is_none());
    assert!(!v.shows("Wide"));
    assert_eq!(islands::favorites_row_bottom(&v.ctx), None);
}

#[test]
fn the_strip_is_not_drawn_in_mouselook() {
    let mut v = with_strip();
    v.hidden = true;
    v.frames(3);
    assert!(v.island().is_none());
    assert!(!v.shows("Sorting"));
    v.hidden = false;
    v.frames(3);
    assert!(v.island().is_some() && v.shows("Sorting"));
}

#[test]
fn a_click_on_a_pill_applies_it_and_it_is_the_one_in_use() {
    let mut v = with_strip();
    assert_eq!(v.in_use(), ["Sorting", "Single"]);
    v.click("Wide");
    v.settle();
    assert_eq!(v.active_layout(), Some("Wide"));
    assert_eq!(v.in_use(), ["Wide", "Sorting"], "a layout leaves the workspace in use");
    assert!(v.saved);
    assert!(v.world.enabled, "the layout's world is back");
    // a workspace by the same path as World › Layouts
    v.open = BTreeSet::from([MAP, PREFERENCES]);
    v.frames(3);
    v.click("Sorting");
    v.settle();
    assert_eq!(v.store.active_workspace.as_deref(), Some("Sorting"));
    let report = apply::report(&v.ctx).expect("an apply was made");
    assert!(report.workspace.is_some(), "the workspace's own report");
    assert!(!v.open.contains(MAP), "the windows it does not list are closed");
}

#[test]
fn what_does_not_fit_is_in_more() {
    let mut v = with_strip();
    v.bars = Some(Bars {
        under: under(190.0),
        landmarks: None,
    });
    v.frames(3);
    let island = v.island().expect("drawn");
    assert!(island.right() <= under(190.0).right() + 1.0, "inside the row's width");
    assert!(v.shows("Wide"));
    assert!(!v.shows("Single"), "not as a pill");
    let more = tr!("fs-strip-more", n = 2).to_string();
    assert!(v.shows(&more), "{:?}", v.texts);
    v.click(&more);
    assert!(v.shows("Sorting") && v.shows("Single"), "the rest is in the menu");
    assert!(v.shows(tr!("fs-layouts-menu-open")) && v.shows(tr!("fs-strip-hide")));
    // "Single" is in use: from the menu, the other layout
    v.click("Wide");
    v.settle();
    assert_eq!(v.active_layout(), Some("Wide"));
    v.click(&more);
    v.click("Single");
    v.settle();
    assert_eq!(v.active_layout(), Some("Single"), "applied from the menu");
    assert!(!v.shows(tr!("fs-strip-hide")), "the menu closed");
}

#[test]
fn more_opens_the_switcher_goes_back_and_hides_the_strip() {
    let mut v = with_strip();
    let island = v.island().expect("drawn");
    // nothing left over: *More…* is its icon alone, at the island's end
    let more = pos2(island.right() - PAD - SLOT / 2.0, island.center().y);
    v.click("Wide");
    v.settle();
    v.click_at(more);
    assert!(v.shows(tr!("fs-layouts-previous")));
    v.click(tr!("fs-layouts-previous"));
    v.settle();
    assert_eq!(v.active_layout(), Some("Single"), "the arrangement of before");
    v.click_at(more);
    v.click(tr!("fs-layouts-menu-open"));
    assert!(crate::folderstorm::workspaces::switcher::is_open(&v.ctx));
    v.ask(Request::Switcher);
    v.saved = false;
    v.click_at(more);
    v.click(tr!("fs-strip-hide"));
    v.frames(2);
    assert!(!v.store.strip);
    assert!(v.saved);
    assert!(v.island().is_none());
}

#[test]
fn without_a_favorite_the_strip_is_more_alone() {
    let mut v = with_strip();
    v.store.favorites.clear();
    v.frames(3);
    let island = v.island().expect("drawn");
    assert!((island.width() - (SLOT + PAD * 2.0)).abs() <= 1.0, "{island:?}");
    v.click_at(island.center());
    assert!(v.shows(tr!("fs-layouts-menu-open")));
}

#[test]
fn on_the_favourites_row_the_landmarks_stop_before_the_strip() {
    let mut v = with_strip();
    v.bars = Some(Bars {
        under: under(900.0),
        landmarks: Some(landmarks(30, false)),
    });
    v.frames(3);
    assert!(v.island().is_none(), "no island of its own: the row holds it");
    let row = favorites::row_bounds(under(900.0), false);
    assert_eq!(islands::favorites_row_bottom(&v.ctx), Some(row.bottom()));
    let lefts = v.lefts(&["Wide", "Sorting", "Single"]);
    assert!(lefts[0] < lefts[1] && lefts[1] < lefts[2], "{lefts:?}");
    assert!(v.text_rect("Single").right() < row.right() - SLOT, "before the button that folds the row");
    assert!(lefts[0] > row.center().x, "in the right half: at most {SHARE} of the row");
    // every landmark drawn, and the row's own "n more", ends before our divider
    let ours = lefts[0] - 6.0 - 14.0 - 5.0;
    let theirs = v
        .texts
        .iter()
        .filter(|(text, rect)| (text.starts_with("Landmark ") || text.ends_with(" more")) && rect.top() < row.bottom())
        .map(|(_, rect)| rect.right())
        .fold(f32::MIN, f32::max);
    assert!(theirs > row.left(), "landmarks are drawn");
    assert!(theirs <= ours - DIVIDER, "{theirs} against {ours}");
    assert!(v.shows("Landmark 0") && !v.shows("Landmark 29"), "the row's own overflow");
    // a click there applies as well
    v.click("Wide");
    v.settle();
    assert_eq!(v.active_layout(), Some("Wide"));
    // without the strip the landmarks have the row to themselves
    let before = v.texts.iter().filter(|(text, _)| text.starts_with("Landmark ")).count();
    v.store.strip = false;
    v.frames(3);
    let after = v.texts.iter().filter(|(text, _)| text.starts_with("Landmark ")).count();
    assert!(after > before, "{after} landmarks against {before}");
    assert!(!v.shows("Sorting"));
}

#[test]
fn beside_the_tab_when_the_favourites_row_is_folded() {
    let mut v = with_strip();
    v.bars = Some(Bars {
        under: under(900.0),
        landmarks: Some(landmarks(4, true)),
    });
    v.frames(3);
    let tab = favorites::row_bounds(under(900.0), true);
    let rect = v.island().expect("drawn");
    assert_eq!(rect.left(), tab.right() + MARGIN);
    assert_eq!(rect.top(), tab.top());
    assert!(v.shows("Wide") && !v.shows("Landmark 0"));
    assert_eq!(islands::favorites_row_bottom(&v.ctx), Some(rect.bottom()));
}

#[test]
fn the_menu_and_the_switcher_show_and_hide_the_strip() {
    let mut v = with_strip();
    v.menu = true;
    v.frames(2);
    v.click(tr!("fs-strip-show"));
    assert!(!v.store.strip, "the check of World › Layouts");
    v.click(tr!("fs-strip-show"));
    assert!(v.store.strip);
    v.menu = false;
    v.open_switcher();
    v.click(tr!("fs-strip-box"));
    v.frames(2);
    assert!(!v.store.strip, "the switcher's box");
    assert!(v.island().is_none());
    // a layout applied does not bring it back: it is no part of one
    v.ask(Request::Apply("Wide".to_owned()));
    v.settle();
    assert!(!v.store.strip);
    v.ask(Request::Strip(true));
    assert!(v.store.strip && v.island().is_some());
}
