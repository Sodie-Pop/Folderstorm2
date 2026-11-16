//! Applying a layout opens the windows it was saved with open, and closes
//! none (the human's report on PR #50).

use super::*;
use crate::folderstorm::workspaces::words::report_text;
use aurora_i18n::tr;

/// Where "Wide" has the world map.
fn map_in_wide() -> Rect {
    Rect::from_min_size(pos2(10.0, 660.0), vec2(400.0, 230.0))
}

fn report(v: &Viewer) -> apply::Report {
    apply::report(&v.ctx).expect("an apply was made")
}

#[test]
fn a_layout_records_which_of_its_windows_are_open() {
    let mut v = two_layouts();
    let open = |v: &Viewer, name: &str| -> Vec<bool> {
        let windows = v.layout(name).windows.as_ref().expect("the windows part");
        [INVENTORY, MINIMAP, PEOPLE, MAP]
            .iter()
            .map(|id| windows.windows[*id].open)
            .collect()
    };
    assert_eq!(open(&v, "Wide"), [true; 4]);
    // a window closed since is still captured, as closed
    v.open.remove(MAP);
    v.frames(3);
    v.save_new("No map", Parts::default()).expect("saved");
    assert_eq!(open(&v, "No map"), [true, true, true, false]);
    let saved = serde_json::to_value(v.layout("No map")).expect("saved");
    assert_eq!(saved["windows"]["windows"][MAP]["open"], false);
    assert_eq!(saved["windows"]["windows"][PEOPLE]["open"], true);
}

#[test]
fn a_closed_window_the_layout_has_open_is_opened_at_its_place() {
    let mut v = two_layouts();
    v.apply("Single");
    v.settle();
    v.open.remove(MAP);
    v.open.remove(INVENTORY);
    v.frames(3);
    v.apply("Wide");
    v.settle();
    assert!(v.open.contains(MAP) && v.open.contains(INVENTORY), "{:?}", v.open);
    assert!(close(v.rect(MAP), map_in_wide()), "{:?}", v.rect(MAP));
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(10.0, 50.0), vec2(500.0, 600.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
    let report = report(&v);
    assert_eq!((report.opened, report.blocked), (2, 0));
    assert!(report_text(&report, 0).contains(&tr!("fs-layouts-applied-opened", count = 2)));
    v.settle();
    assert!(v.store.pending.is_empty(), "nothing waits for its place");
    assert!(!v.modified(), "opening its windows is not a change");
    // nothing to open the second time
    v.apply("Wide");
    v.settle();
    assert_eq!(self::report(&v).opened, 0);
}

#[test]
fn a_layout_never_closes_a_window_nor_opens_one_it_has_closed() {
    let mut v = two_layouts();
    saved_closed(&mut v, "Wide", MAP);
    saved_closed(&mut v, "Wide", PEOPLE);
    // the map closed, People open: both stay as they are
    v.open.remove(MAP);
    v.frames(3);
    v.apply("Wide");
    v.settle();
    assert!(!v.open.contains(MAP), "saved closed: not opened");
    assert!(v.open.contains(PEOPLE), "saved closed: not closed");
    assert!(v.open.contains(PREFERENCES), "not a window of the layout");
    assert_eq!(report(&v).opened, 0);
    assert!(v.store.pending.contains(MAP), "its place waits for it");
}

#[test]
fn a_window_that_cannot_be_opened_now_is_left_closed_and_said() {
    let mut v = two_layouts();
    v.open.remove(MAP);
    v.open.remove(INVENTORY);
    // (as RLVa's @showinv has Inventory)
    v.blocked.insert(INVENTORY);
    v.frames(3);
    v.apply("Wide");
    v.settle();
    assert!(v.open.contains(MAP) && !v.open.contains(INVENTORY));
    let report = report(&v);
    assert_eq!((report.opened, report.blocked), (1, 1));
    let text = report_text(&report, 0);
    assert!(text.contains(&tr!("fs-layouts-applied-blocked", count = 1)), "{text}");
    // the rest of the layout is applied all the same
    assert!(close(v.rect(MAP), map_in_wide()), "{:?}", v.rect(MAP));
    assert_eq!(v.world, v.layout("Wide").world.clone().expect("a world part"));
    // and its place waits: opened by hand later, it goes there
    assert!(v.store.pending.contains(INVENTORY));
    v.blocked.clear();
    v.open.insert(INVENTORY);
    v.settle();
    assert!(
        close(v.rect(INVENTORY), Rect::from_min_size(pos2(10.0, 50.0), vec2(500.0, 600.0))),
        "{:?}",
        v.rect(INVENTORY)
    );
}

#[test]
fn a_layout_saved_before_the_open_state_opens_nothing() {
    let mut v = two_layouts();
    // through the file, without the field: as an older build wrote it
    let mut file = serde_json::to_value(&v.store).expect("saved");
    for layout in file["layouts"].as_array_mut().expect("layouts") {
        for (_, window) in layout["windows"]["windows"].as_object_mut().expect("windows") {
            window.as_object_mut().expect("a window").remove("open").expect("the field");
        }
    }
    v.store = serde_json::from_value(file).expect("read");
    v.open.remove(MAP);
    v.frames(3);
    v.apply("Wide");
    v.settle();
    assert!(!v.open.contains(MAP));
    assert_eq!(report(&v).opened, 0);
    assert!(v.store.pending.contains(MAP), "placed when opened, as before");
}

#[test]
fn a_window_saved_open_and_minimized_is_opened_minimized() {
    let mut v = two_layouts();
    widgets::minimize_floater(&v.ctx, PEOPLE);
    v.frames(4);
    v.save_new("People small", Parts::default()).expect("saved");
    let people = v.layout("People small").windows.as_ref().expect("windows").windows[PEOPLE];
    assert!(people.minimized && people.open);
    v.apply("Single");
    v.settle();
    v.open.remove(PEOPLE);
    v.frames(3);
    v.apply("People small");
    v.settle();
    assert!(v.open.contains(PEOPLE));
    assert!(widgets::floater_minimized(&v.ctx, PEOPLE));
    assert!((v.rect(PEOPLE).min - pos2(people.x, people.y)).abs().max_elem() <= 1.0);
}

#[test]
fn previous_opens_what_was_open_before_and_closes_nothing() {
    let mut v = two_layouts();
    v.apply("Wide");
    v.settle();
    let before = v.rect(MAP);
    // "Single" applied with the map open, then the map closed by hand
    v.apply("Single");
    v.settle();
    v.open.remove(MAP);
    v.frames(3);
    assert!(v.previous());
    v.settle();
    assert!(v.open.contains(MAP), "it was open in the arrangement before");
    assert!(close(v.rect(MAP), before), "{:?}", v.rect(MAP));
    assert_eq!(v.open.len(), 5, "and nothing was closed: {:?}", v.open);
}
