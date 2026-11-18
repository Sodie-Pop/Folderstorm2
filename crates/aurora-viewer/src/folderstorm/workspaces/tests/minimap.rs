//! The mini-map, a window without a title bar (`bare_window`), takes the
//! place and the size its layout has, open or closed when the layout is
//! applied (the human's report: it opened where it was before).

use super::*;

/// A layout of the windows of now, with the mini-map at `rect`.
fn with_minimap(v: &mut Viewer, name: &str, rect: Rect) {
    let mut windows = capture::windows(&v.ctx).expect("windows");
    windows.windows.insert(
        MINIMAP.into(),
        Placement {
            x: rect.min.x,
            y: rect.min.y,
            width: rect.width(),
            height: rect.height(),
            minimized: false,
            open: true,
        },
    );
    v.store.add(Layout::new(name, None, Some(windows), Vec::new())).expect("added");
}

fn corner() -> Rect {
    Rect::from_min_size(pos2(40.0, 620.0), vec2(260.0, 240.0))
}

fn elsewhere() -> Rect {
    Rect::from_min_size(pos2(700.0, 120.0), vec2(150.0, 330.0))
}

/// Two layouts of the mini-map, "Corner" and "Elsewhere", the second
/// applied.
fn two_places() -> Viewer {
    let mut v = Viewer::new(1600.0, 900.0);
    v.frames(4);
    with_minimap(&mut v, "Corner", corner());
    with_minimap(&mut v, "Elsewhere", elsewhere());
    v.apply("Elsewhere");
    v.settle();
    assert!(close(v.rect(MINIMAP), elsewhere()), "{:?}", v.rect(MINIMAP));
    v
}

#[test]
fn an_open_mini_map_takes_its_place_and_size_and_keeps_them() {
    let mut v = two_places();
    v.apply("Corner");
    v.settle();
    assert!(close(v.rect(MINIMAP), corner()), "{:?}", v.rect(MINIMAP));
    v.settle();
    assert!(close(v.rect(MINIMAP), corner()), "it stays: {:?}", v.rect(MINIMAP));
    assert!(!v.modified(), "taking its place is not a change");
}

#[test]
fn a_closed_mini_map_is_opened_at_its_place_and_size() {
    let mut v = two_places();
    v.open.remove(MINIMAP);
    v.frames(3);
    v.apply("Corner");
    v.settle();
    assert!(v.open.contains(MINIMAP), "opened");
    assert!(close(v.rect(MINIMAP), corner()), "{:?}", v.rect(MINIMAP));
    v.settle();
    assert!(close(v.rect(MINIMAP), corner()), "it stays: {:?}", v.rect(MINIMAP));
    assert!(v.store.pending.is_empty(), "nothing waits for its place");
    assert!(!v.modified(), "opening it is not a change");
}

#[test]
fn a_mini_map_closed_for_a_long_time_is_opened_at_its_place_and_size() {
    let mut v = two_places();
    v.open.remove(MINIMAP);
    v.settle();
    v.settle();
    v.apply("Corner");
    v.settle();
    assert!(close(v.rect(MINIMAP), corner()), "{:?}", v.rect(MINIMAP));
}

/// The viewer started again with the egui memory `ui/layout.rs` restores
/// (`ui_layout.ron`): the windows' positions and our records, but no window
/// size, which egui does not keep.
fn restored(v: &Viewer) -> Viewer {
    let next = v.restarted();
    let saved = ron::to_string(&v.ctx.memory(|m| m.clone())).expect("saved");
    let memory: egui::Memory = ron::from_str(&saved).expect("read");
    let options = next.ctx.options(|o| o.clone());
    next.ctx.memory_mut(|m| *m = memory);
    next.ctx.options_mut(|o| *o = options);
    assert!(
        egui::AreaState::load(&next.ctx, egui::Id::unique(MINIMAP)).is_some_and(|state| state.size.is_none()),
        "a place and no size, as after a restart"
    );
    next
}

#[test]
fn a_mini_map_not_shown_since_the_start_is_opened_at_its_place_and_size() {
    let mut v = two_places();
    v.open.remove(MINIMAP);
    v.frames(3);
    let mut next = restored(&v);
    next.frames(4);
    next.apply("Corner");
    next.settle();
    assert!(next.open.contains(MINIMAP), "opened");
    assert!(close(next.rect(MINIMAP), corner()), "{:?}", next.rect(MINIMAP));
    next.settle();
    assert!(close(next.rect(MINIMAP), corner()), "it stays: {:?}", next.rect(MINIMAP));
    assert!(next.store.pending.is_empty(), "nothing waits for its place");
}

#[test]
fn a_layout_saved_with_the_mini_map_closed_since_the_start_has_its_size() {
    let mut v = two_places();
    v.open.remove(MINIMAP);
    v.frames(3);
    let mut next = restored(&v);
    next.frames(4);
    next.save_new("Closed", Parts::default()).expect("saved");
    let saved = next.layout("Closed").windows.as_ref().expect("the windows part").windows[MINIMAP];
    let rect = Rect::from_min_size(pos2(saved.x, saved.y), vec2(saved.width, saved.height));
    assert!(close(rect, elsewhere()), "{rect:?}");
    assert!(!saved.open, "closed");
}

/// The user's hand: a press at `from`, a drag to `to` over ten frames and
/// the release there.
fn drag(v: &mut Viewer, from: Pos2, to: Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    v.events = vec![egui::Event::PointerMoved(from)];
    v.frame();
    v.events = vec![button(from, true)];
    v.frame();
    for step in 1..=10 {
        v.events = vec![egui::Event::PointerMoved(from + (to - from) * (step as f32 / 10.0))];
        v.frame();
    }
    v.events = vec![button(to, false)];
    v.frame();
    v.frames(3);
}

/// The mini-map moved by its surface and resized by its corner, by hand.
fn by_hand(v: &mut Viewer) -> Rect {
    let was = v.rect(MINIMAP);
    drag(v, was.center(), was.center() + vec2(-300.0, 200.0));
    let moved = v.rect(MINIMAP);
    assert!(close(moved, was.translate(vec2(-300.0, 200.0))), "moved: {moved:?}");
    drag(v, moved.max - vec2(3.0, 3.0), moved.max + vec2(80.0, -60.0));
    let resized = v.rect(MINIMAP);
    assert_eq!(resized.min, moved.min, "resized by its corner");
    assert!(
        resized.width() > moved.width() + 50.0 && resized.height() < moved.height() - 30.0,
        "resized: {resized:?}"
    );
    v.settle();
    resized
}

#[test]
fn a_mini_map_placed_by_hand_is_saved_and_put_back_there() {
    let mut v = two_places();
    let placed = by_hand(&mut v);
    v.save_new("Hand", Parts::default()).expect("saved");
    let saved = v.layout("Hand").windows.as_ref().expect("the windows part").windows[MINIMAP];
    let rect = Rect::from_min_size(pos2(saved.x, saved.y), vec2(saved.width, saved.height));
    assert!(close(rect, placed), "saved {rect:?}, placed {placed:?}");
    // moved by hand again, then the layout in use applied again
    let r = v.rect(MINIMAP);
    drag(&mut v, r.center(), r.center() + vec2(200.0, -100.0));
    assert!(!close(v.rect(MINIMAP), placed), "moved away");
    v.apply("Hand");
    v.settle();
    assert!(close(v.rect(MINIMAP), placed), "{:?}", v.rect(MINIMAP));
    // another layout, then this one with the mini-map closed
    v.apply("Corner");
    v.settle();
    v.open.remove(MINIMAP);
    v.frames(3);
    v.apply("Hand");
    v.settle();
    assert!(v.open.contains(MINIMAP), "opened");
    assert!(close(v.rect(MINIMAP), placed), "{:?}", v.rect(MINIMAP));
}

/// A workspace keeps no place of its own: the mini-map takes the place of
/// the layout it names (plans/workspaces.md, checkpoint 6).
#[test]
fn a_workspace_puts_the_mini_map_where_its_layout_has_it() {
    let mut v = two_places();
    let placed = by_hand(&mut v);
    v.save_new("Hand", Parts::default()).expect("saved");
    let keep = apply::Keep {
        layout: true,
        folders: false,
    };
    let ctx = v.ctx.clone();
    apply::save_new_workspace(&ctx, &mut v.live(), "Map", keep).expect("saved");
    v.apply("Corner");
    v.settle();
    v.open.remove(MINIMAP);
    v.frames(3);
    let (index, _) = v.store.find_workspace("Map").expect("in the list");
    apply::apply_workspace(&ctx, &mut v.live(), index).expect("applied");
    v.settle();
    assert!(v.open.contains(MINIMAP), "opened by the workspace");
    assert!(close(v.rect(MINIMAP), placed), "{:?}", v.rect(MINIMAP));
}

#[test]
fn saving_records_the_mini_map_where_it_is() {
    let mut v = two_places();
    v.save_new("Saved", Parts::default()).expect("saved");
    let saved = v.layout("Saved").windows.as_ref().expect("the windows part").windows[MINIMAP];
    let rect = Rect::from_min_size(pos2(saved.x, saved.y), vec2(saved.width, saved.height));
    assert!(close(rect, elsewhere()), "{rect:?}");
    v.apply("Corner");
    v.settle();
    v.apply("Saved");
    v.settle();
    assert!(close(v.rect(MINIMAP), elsewhere()), "{:?}", v.rect(MINIMAP));
}
