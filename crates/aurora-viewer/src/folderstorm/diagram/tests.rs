use super::*;

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

fn m(x: i32, y: i32, w: i32, h: i32) -> Monitor {
    Monitor {
        rect: [x, y, w, h],
        scale: 1.0,
    }
}

/// Two 1920x1080 monitors side by side, the window stretched over both.
fn two() -> Desktop {
    Desktop {
        window: (3840, 1080),
        position: Some((0, 0)),
        monitors: vec![m(0, 0, 1920, 1080), m(1920, 0, 1920, 1080)],
    }
}

fn on(left: u32, right: u32, top: u32, bottom: u32) -> ViewportSettings {
    ViewportSettings {
        enabled: true,
        left,
        right,
        top,
        bottom,
        ..Default::default()
    }
}

fn margins(left: u32, right: u32, top: u32, bottom: u32) -> Margins {
    Margins { left, right, top, bottom }
}

#[test]
fn the_desktop_is_scaled_into_the_diagram_and_back() {
    let bounds = r(0.0, 0.0, 3840.0, 1080.0);
    let room = r(100.0, 50.0, 400.0, 200.0);
    let map = Mapping::fit(bounds, room, 10.0);
    // limited by the width: 380 points for 3840 pixels
    assert!((map.scale - 380.0 / 3840.0).abs() < 1e-6, "{}", map.scale);
    let drawn = map.rect_to_diagram(bounds);
    assert!(
        (drawn.left() - 110.0).abs() < 1e-3 && (drawn.right() - 490.0).abs() < 1e-3,
        "{drawn:?}"
    );
    assert!((drawn.center().y - room.center().y).abs() < 1e-3, "centred: {drawn:?}");
    for p in [pos2(0.0, 0.0), pos2(1920.0, 540.0), pos2(-1920.0, -200.0), pos2(3840.0, 1080.0)] {
        let back = map.to_window(map.to_diagram(p));
        assert!((back - p).length() < 0.01, "{p:?} -> {back:?}");
    }
    // a tall desktop is limited by the height, centred across
    let tall = Mapping::fit(r(-1080.0, -500.0, 1080.0, 1920.0), room, 10.0);
    assert!((tall.scale - 180.0 / 1920.0).abs() < 1e-6);
    let drawn = tall.rect_to_diagram(r(-1080.0, -500.0, 1080.0, 1920.0));
    assert!((drawn.center().x - room.center().x).abs() < 1e-3 && (drawn.top() - 60.0).abs() < 1e-3);
    // an empty desktop does not divide by zero
    let none = Mapping::fit(Rect::NOTHING.union(r(0.0, 0.0, 0.0, 0.0)), room, 10.0);
    assert!(none.scale.is_finite() && none.scale > 0.0);
}

#[test]
fn the_diagram_height_follows_the_desktop_within_limits() {
    assert_eq!(height_for(600.0, r(0.0, 0.0, 3840.0, 1080.0)), 168.75);
    assert_eq!(height_for(600.0, r(0.0, 0.0, 7680.0, 1080.0)), DIAGRAM_MIN_H);
    assert_eq!(height_for(600.0, r(0.0, 0.0, 1080.0, 1920.0)), DIAGRAM_MAX_H);
}

#[test]
fn a_monitor_left_of_and_above_the_main_one_lands_at_negative_window_coordinates() {
    // the main monitor at 0,0; a second one left of it, 200 px higher; the
    // window over the right half of the left one and the main one
    let desktop = Desktop {
        window: (2880, 1080),
        position: Some((-960, 0)),
        monitors: vec![m(0, 0, 1920, 1080), m(-1920, -200, 1920, 1440)],
    };
    let inside = desktop.monitors_in_window();
    assert_eq!(inside, vec![r(960.0, 0.0, 1920.0, 1080.0), r(-960.0, -200.0, 1920.0, 1440.0)]);
    assert_eq!(desktop.bounds(), Rect::from_min_max(pos2(-960.0, -200.0), pos2(2880.0, 1240.0)));
    assert_eq!(desktop.monitor_at(pos2(10.0, 500.0)), Some(1));
    assert_eq!(desktop.monitor_at(pos2(-500.0, -100.0)), Some(1), "above the window, still on it");
    assert_eq!(desktop.monitor_at(pos2(1000.0, 500.0)), Some(0));
    assert_eq!(desktop.monitor_at(pos2(3000.0, 500.0)), None);
    // the main one: everything left of x = 960 in the window is margin
    assert_eq!(desktop.fit_monitor(0), Fit::Margins(margins(960, 0, 0, 0)));
    // the left one covers the left third, top to bottom
    assert_eq!(desktop.fit_monitor(1), Fit::Margins(margins(0, 1920, 0, 0)));
    // the borders between are where edges snap
    assert_eq!(desktop.snap_targets(Edge::Left), vec![0.0, 960.0, 2880.0]);
    assert_eq!(desktop.snap_targets(Edge::Top), vec![0.0, 1080.0]);
}

#[test]
fn unequal_monitors_show_whole_and_snap_at_their_borders() {
    // a 1920x1080 monitor and a 2560x1440 one, 180 px higher, the window
    // 4480x1080 over both
    let desktop = Desktop {
        window: (4480, 1080),
        position: Some((0, 0)),
        monitors: vec![
            m(0, 0, 1920, 1080),
            Monitor {
                rect: [1920, -180, 2560, 1440],
                scale: 1.5,
            },
        ],
    };
    assert_eq!(desktop.bounds(), Rect::from_min_max(pos2(0.0, -180.0), pos2(4480.0, 1260.0)));
    assert_eq!(desktop.snap_targets(Edge::Right), vec![0.0, 1920.0, 4480.0]);
    assert_eq!(
        desktop.snap_targets(Edge::Bottom),
        vec![0.0, 1080.0],
        "the taller one's borders are outside"
    );
    // the large monitor holds the whole height of the window
    assert_eq!(desktop.fit_monitor(1), Fit::Margins(margins(1920, 0, 0, 0)));
    // its scale changes nothing to the margins, physical pixels
    let mut flat = desktop.clone();
    flat.monitors[1].scale = 1.0;
    assert_eq!(flat.fit_monitor(1), desktop.fit_monitor(1));
}

#[test]
fn putting_the_world_on_a_monitor_refuses_what_is_not_under_the_window() {
    let mut desktop = two();
    desktop.monitors.push(m(5000, 0, 1920, 1080));
    assert_eq!(desktop.fit_monitor(0), Fit::Margins(margins(0, 1920, 0, 0)));
    assert_eq!(desktop.fit_monitor(1), Fit::Margins(margins(1920, 0, 0, 0)));
    assert_eq!(desktop.fit_monitor(2), Fit::NotUnder, "away from the window");
    assert_eq!(desktop.fit_monitor(9), Fit::NotUnder, "no such monitor");
    // the window barely over a third monitor: a 100 px sliver would be set
    // aside at once
    let sliver = Desktop {
        window: (2020, 1080),
        position: Some((0, 0)),
        monitors: vec![m(0, 0, 1920, 1080), m(1920, 0, 1920, 1080)],
    };
    assert_eq!(sliver.fit_monitor(1), Fit::TooSmall);
    // one monitor under the whole window: no margins at all
    let single = Desktop {
        window: (1920, 1080),
        position: Some((0, 0)),
        monitors: vec![m(0, 0, 1920, 1080)],
    };
    assert_eq!(single.fit_monitor(0), Fit::Margins(Margins::default()));
}

#[test]
fn without_a_window_position_the_window_is_drawn_alone() {
    // Wayland: the monitors are known, the window's place is not
    let desktop = Desktop {
        window: (2560, 1440),
        position: None,
        monitors: vec![m(0, 0, 2560, 1440)],
    };
    assert!(desktop.monitors_in_window().is_empty());
    assert_eq!(desktop.bounds(), r(0.0, 0.0, 2560.0, 1440.0));
    assert_eq!(desktop.monitor_at(pos2(100.0, 100.0)), None);
    assert_eq!(desktop.fit_monitor(0), Fit::NotUnder);
    assert_eq!(desktop.first_selection(desktop.window_rect()), None);
    assert_eq!(desktop.monitor_layout(), None, "no Fit to a monitor either");
    assert_eq!(desktop.snap_targets(Edge::Left), vec![0.0, 2560.0], "the window's sides only");
    // the edges still drag
    let dragged = drag(
        desktop.window,
        Margins::default(),
        Edge::Left,
        800.0,
        &desktop.snap_targets(Edge::Left),
        20.0,
    );
    assert_eq!(dragged, margins(800, 0, 0, 0));
}

#[test]
fn the_first_selection_is_the_monitor_holding_the_world() {
    let desktop = two();
    assert_eq!(desktop.first_selection(r(1920.0, 0.0, 1920.0, 1080.0)), Some(1));
    assert_eq!(desktop.first_selection(r(0.0, 0.0, 1000.0, 1080.0)), Some(0));
    // a tie (the world across both halves) keeps the first
    assert_eq!(desktop.first_selection(r(960.0, 0.0, 1920.0, 1080.0)), Some(0));
    // a world off every monitor: the one holding most of the window
    let mut lopsided = two();
    lopsided.monitors[0] = m(0, 0, 1000, 1080);
    assert_eq!(lopsided.first_selection(r(1200.0, 0.0, 0.0, 0.0)), Some(1));
}

#[test]
fn an_edge_is_grabbed_near_its_line() {
    let map = Mapping {
        scale: 0.1,
        origin: pos2(0.0, 0.0),
    };
    // the world on the right monitor: x 192..384, y 0..108 in the diagram
    let world = r(1920.0, 0.0, 1920.0, 1080.0);
    assert_eq!(edge_at(&map, world, pos2(193.0, 54.0)), Some(Edge::Left));
    assert_eq!(edge_at(&map, world, pos2(192.0 - GRAB, 54.0)), Some(Edge::Left));
    assert_eq!(edge_at(&map, world, pos2(192.0 - GRAB - 1.0, 54.0)), None);
    assert_eq!(edge_at(&map, world, pos2(380.0, 54.0)), Some(Edge::Right));
    assert_eq!(edge_at(&map, world, pos2(300.0, 2.0)), Some(Edge::Top));
    assert_eq!(edge_at(&map, world, pos2(300.0, 107.0)), Some(Edge::Bottom));
    assert_eq!(edge_at(&map, world, pos2(290.0, 54.0)), None, "the middle");
    // a corner goes to the nearer line
    assert_eq!(edge_at(&map, world, pos2(193.0, 3.0)), Some(Edge::Left));
    assert_eq!(edge_at(&map, world, pos2(195.0, 1.0)), Some(Edge::Top));
    // past the end of a line, beyond the grab distance: nothing
    assert_eq!(edge_at(&map, world, pos2(193.0, 120.0)), None);
}

#[test]
fn dragging_each_edge_sets_its_margin_and_leaves_the_others() {
    let size = (3840, 1080);
    let start = margins(100, 200, 10, 20);
    let none: &[f32] = &[];
    assert_eq!(drag(size, start, Edge::Left, 1500.4, none, 0.0), margins(1500, 200, 10, 20));
    assert_eq!(drag(size, start, Edge::Right, 3000.0, none, 0.0), margins(100, 840, 10, 20));
    assert_eq!(drag(size, start, Edge::Top, 300.0, none, 0.0), margins(100, 200, 300, 20));
    assert_eq!(drag(size, start, Edge::Bottom, 900.0, none, 0.0), margins(100, 200, 10, 180));
}

#[test]
fn a_dragged_edge_snaps_to_monitor_borders_and_the_window_sides() {
    let desktop = two();
    let size = desktop.window;
    let targets = desktop.snap_targets(Edge::Left);
    assert_eq!(targets, vec![0.0, 1920.0, 3840.0]);
    let start = Margins::default();
    assert_eq!(drag(size, start, Edge::Left, 1914.0, &targets, 8.0), margins(1920, 0, 0, 0));
    assert_eq!(drag(size, start, Edge::Left, 1929.0, &targets, 8.0).left, 1929, "beyond the snap");
    assert_eq!(drag(size, start, Edge::Left, 5.0, &targets, 8.0).left, 0, "the window's side");
    assert_eq!(drag(size, start, Edge::Right, 1925.0, &targets, 8.0), margins(0, 1920, 0, 0));
    // the nearer of two targets wins
    assert_eq!(drag(size, start, Edge::Left, 1000.0, &[990.0, 1004.0], 20.0).left, 1004);
}

#[test]
fn a_drag_stops_before_the_world_gets_under_a_quarter() {
    let size = (3840, 1080);
    let none: &[f32] = &[];
    // a quarter of 3840 is 960: the left margin stops at 2880
    assert_eq!(drag(size, Margins::default(), Edge::Left, 3500.0, none, 0.0).left, 2880);
    // with a right margin, sooner
    assert_eq!(drag(size, margins(0, 1000, 0, 0), Edge::Left, 3500.0, none, 0.0).left, 1880);
    // the right edge pushed left of the left margin
    assert_eq!(drag(size, margins(2000, 0, 0, 0), Edge::Right, 100.0, none, 0.0).right, 880);
    // the height: a quarter of 1080 is 270
    assert_eq!(drag(size, Margins::default(), Edge::Bottom, -50.0, none, 0.0).bottom, 810);
    // out of the window on the outside: no margin, never negative
    assert_eq!(drag(size, margins(500, 0, 0, 0), Edge::Left, -300.0, none, 0.0).left, 0);
    assert_eq!(drag(size, margins(0, 500, 0, 0), Edge::Right, 9000.0, none, 0.0).right, 0);
    assert_eq!(drag(size, margins(0, 0, 0, 0), Edge::Left, f32::NAN, none, 0.0).left, 0);
    // a snap target past the limit is not followed past it
    assert_eq!(drag(size, Margins::default(), Edge::Left, 2890.0, &[2895.0], 10.0).left, 2880);
    // an opposite margin already past the window (set with the sliders)
    assert_eq!(drag(size, margins(0, 16384, 0, 0), Edge::Left, 1000.0, none, 0.0).left, 0);
    // what a drag gives is always kept by the viewport
    for at in [0.0, 500.0, 1919.0, 2879.0, 2880.0, 2881.0, 3839.0] {
        let m = drag(size, margins(0, 300, 0, 0), Edge::Left, at, none, 0.0);
        assert!(
            viewport::world_rect(size, &m.on(&on(0, 0, 0, 0)), Overrides::default()).is_some(),
            "{at}: {m:?}"
        );
    }
}

#[test]
fn the_world_off_whole_set_aside_or_in_its_rectangle() {
    let size = (3840, 1080);
    assert_eq!(world(size, &ViewportSettings::default()), World::Off);
    assert_eq!(world(size, &on(0, 0, 0, 0)), World::Whole);
    assert_eq!(world(size, &on(1920, 0, 0, 0)), World::Custom(r(1920.0, 0.0, 1920.0, 1080.0)));
    // the quarter-of-the-window fallback: a window shrunk to one monitor
    assert_eq!(world((1920, 1080), &on(1920, 0, 0, 0)), World::SetAside);
    assert_eq!(world((2000, 1080), &on(1920, 0, 0, 0)), World::SetAside);
    assert_eq!(world((0, 0), &on(1920, 0, 0, 0)), World::Whole, "no window yet");
}

#[test]
fn the_asked_rectangle_stays_in_the_window() {
    let size = (1920, 1080);
    assert_eq!(
        asked_rect(size, margins(100, 200, 10, 20)),
        Rect::from_min_max(pos2(100.0, 10.0), pos2(1720.0, 1060.0))
    );
    // margins larger than the window: an empty rectangle at its side
    let set_aside = asked_rect(size, margins(1920, 0, 0, 0));
    assert_eq!(set_aside, Rect::from_min_max(pos2(1920.0, 0.0), pos2(1920.0, 1080.0)));
    let crossed = asked_rect(size, margins(1500, 1500, 0, 0));
    assert_eq!((crossed.left(), crossed.right()), (1500.0, 1500.0));
    let huge = asked_rect(size, margins(u32::MAX, u32::MAX, u32::MAX, u32::MAX));
    assert!(huge.left() <= huge.right() && huge.right() <= 1920.0 && huge.bottom() <= 1080.0);
}

#[test]
fn demo_monitors_side_by_side_or_placed() {
    let three = parse_demo_monitors("1920x1080,1920x1080,1920x1080").expect("three monitors");
    let DemoMonitors::Layout { monitors, window_at } = &three else {
        panic!("{three:?}")
    };
    assert_eq!(
        monitors.iter().map(|m| m.rect).collect::<Vec<_>>(),
        vec![[0, 0, 1920, 1080], [1920, 0, 1920, 1080], [3840, 0, 1920, 1080]]
    );
    assert_eq!(*window_at, None);
    let desktop = three.desktop((5760, 1080));
    assert_eq!(desktop.position, Some((0, 0)));
    assert_eq!(desktop.fit_monitor(1), Fit::Margins(margins(1920, 1920, 0, 0)));

    // a monitor left of the main one, a scale, spaces and semicolons
    let placed = parse_demo_monitors(" 1920x1080-1920+120@150% ; 2560x1440+0+0@1.25 ").expect("placed");
    let DemoMonitors::Layout { monitors, .. } = &placed else {
        panic!("{placed:?}")
    };
    assert_eq!(
        monitors[0],
        Monitor {
            rect: [-1920, 120, 1920, 1080],
            scale: 1.5
        }
    );
    assert_eq!(
        monitors[1],
        Monitor {
            rect: [0, 0, 2560, 1440],
            scale: 1.25
        }
    );
    // the window at the corner of them all by default, or where it is put
    assert_eq!(placed.desktop((4480, 1080)).position, Some((-1920, 0)));
    let moved = parse_demo_monitors("1920x1080,1920x1080,window+960-20").expect("window placed");
    assert_eq!(moved.desktop((1920, 1080)).position, Some((960, -20)));
    // a monitor without a place after a placed one goes right of it
    let after = parse_demo_monitors("1280x1024+0+56,1920x1080").expect("after");
    let DemoMonitors::Layout { monitors, .. } = &after else {
        panic!("{after:?}")
    };
    assert_eq!(monitors[1].rect, [1280, 56, 1920, 1080]);
}

#[test]
fn demo_monitors_wayland_and_errors() {
    assert_eq!(parse_demo_monitors("wayland"), Ok(DemoMonitors::NoPosition));
    assert_eq!(parse_demo_monitors("None"), Ok(DemoMonitors::NoPosition));
    assert_eq!(DemoMonitors::NoPosition.desktop((800, 600)), Desktop::window_only((800, 600)));
    for bad in [
        "",
        "abc",
        "1920",
        "1920x",
        "1920x1080+5",
        "1920x1080+5+5+5",
        "1920x1080@",
        "1920x1080@9",
        "1920x1080@-1",
        "0x1080",
        "1920x1080,window",
        "window+0+0",
        "1920x1080+99999999999+0",
    ] {
        assert!(parse_demo_monitors(bad).is_err(), "`{bad}` accepted");
    }
}

#[test]
fn the_desktop_falls_back_to_the_window_alone() {
    let ctx = egui::Context::default();
    let mut seen = None;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(r(0.0, 0.0, 1200.0, 800.0)),
            ..Default::default()
        },
        |ui| {
            seen = Some(desktop(ui.ctx()));
            set_desktop(ui.ctx(), two());
        },
    );
    output.textures_delta.clear();
    let seen = seen.expect("ran");
    assert_eq!(seen.position, None);
    assert!(seen.monitors.is_empty());
    let ppp = ctx.pixels_per_point();
    assert_eq!(seen.window, ((1200.0 * ppp).round() as u32, (800.0 * ppp).round() as u32));
    assert_eq!(desktop(&ctx), two(), "then what the app published");
}

/// An egui frame of the diagram: dragging the world's left edge moves the
/// left margin and snaps it to the border between the monitors; Escape puts
/// it back; a double click on a monitor puts the world on it.
#[test]
fn the_diagram_drags_an_edge_and_puts_the_world_on_a_monitor() {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let p = crate::theme::Theme::default().palette();
    let desktop = two();
    let mut s = on(0, 0, 0, 0);
    let screen = r(0.0, 0.0, 900.0, 700.0);
    let frame = |s: &mut ViewportSettings, events: Vec<egui::Event>| {
        let mut room = (Pos2::ZERO, 0.0);
        let mut changed = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                room = (ui.cursor().min, ui.available_width());
                changed = widget::show(ui, &p, s, &desktop);
            },
        );
        output.textures_delta.clear();
        let (corner, width) = room;
        let bounds = desktop.bounds();
        let rect = Rect::from_min_size(corner, vec2(width, height_for(width, bounds)));
        (Mapping::fit(bounds, rect, widget::PAD), changed)
    };
    let (map, _) = frame(&mut s, vec![]);
    let at = |x: f32, y: f32| map.to_diagram(pos2(x, y));
    let moved = |pos| egui::Event::PointerMoved(pos);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };

    // grab the left edge (the window's side while there are no margins)
    let grab = at(0.0, 540.0) + vec2(1.0, 0.0);
    frame(&mut s, vec![moved(grab)]);
    frame(&mut s, vec![button(grab, true)]);
    // drag it near the border between the monitors: it snaps there
    let near = at(1900.0, 540.0) + vec2(1.0, 0.0);
    frame(&mut s, vec![moved(grab + vec2(12.0, 0.0))]);
    let (_, changed) = frame(&mut s, vec![moved(near)]);
    frame(&mut s, vec![moved(near)]);
    assert!(changed, "the drag changed the margins");
    assert_eq!((s.left, s.right, s.top, s.bottom), (1920, 0, 0, 0), "snapped to the monitor border");
    // further, past a quarter of the window: it stops there
    frame(&mut s, vec![moved(at(3700.0, 540.0))]);
    assert_eq!(s.left, 2880);
    // Escape puts the margin back, and the rest of the drag does nothing
    frame(
        &mut s,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(s.left, 0);
    frame(&mut s, vec![moved(at(1000.0, 540.0))]);
    assert_eq!(s.left, 0, "cancelled");
    frame(&mut s, vec![button(at(1000.0, 540.0), false)]);

    // a double click on the right monitor puts the world on it
    let right = at(2880.0, 300.0);
    frame(&mut s, vec![moved(right)]);
    frame(&mut s, vec![button(right, true), button(right, false)]);
    frame(&mut s, vec![button(right, true), button(right, false)]);
    frame(&mut s, vec![]);
    assert_eq!((s.left, s.right, s.top, s.bottom), (1920, 0, 0, 0));

    // and the world's right edge, dragged, snaps to the window's side
    s.right = 300;
    frame(&mut s, vec![]);
    let edge = at(3540.0, 540.0);
    frame(&mut s, vec![moved(edge)]);
    frame(&mut s, vec![button(edge, true)]);
    frame(&mut s, vec![moved(edge + vec2(12.0, 0.0))]);
    frame(&mut s, vec![moved(at(3838.0, 540.0))]);
    frame(&mut s, vec![button(at(3838.0, 540.0), false)]);
    assert_eq!(s.right, 0);
}

#[test]
fn the_bars_are_drawn_where_the_settings_lay_them_out() {
    let window = Rect::from_min_size(Pos2::ZERO, vec2(3840.0, 1080.0));
    // the world on the right monitor, a little under the window's top
    let world = Rect::from_min_max(pos2(1920.0, 30.0), pos2(3840.0, 1080.0));
    let bars = Thickness {
        left: 60.0,
        right: 0.0,
        top: 80.0,
        bottom: 64.0,
    };
    let strips = |place| bars_strips(window, world, place, bars);
    // inside the world: along its own edges
    assert_eq!(
        strips(BarsPlace::Inside),
        [
            (Edge::Top, Rect::from_min_max(pos2(1920.0, 30.0), pos2(3840.0, 110.0))),
            (Edge::Bottom, Rect::from_min_max(pos2(1920.0, 1016.0), pos2(3840.0, 1080.0))),
            (Edge::Left, Rect::from_min_max(pos2(1920.0, 110.0), pos2(1980.0, 1016.0))),
        ]
    );
    // over the whole window
    assert_eq!(
        strips(BarsPlace::Window),
        [
            (Edge::Top, Rect::from_min_max(pos2(0.0, 0.0), pos2(3840.0, 80.0))),
            (Edge::Bottom, Rect::from_min_max(pos2(0.0, 1016.0), pos2(3840.0, 1080.0))),
            (Edge::Left, Rect::from_min_max(pos2(0.0, 80.0), pos2(60.0, 1016.0))),
        ]
    );
    // around: the left margin has room for the side bar, which goes out of
    // the world; the top margin (30 pixels) has none for the top bars, and
    // there is no bottom margin: those two stay inside
    assert_eq!(
        strips(BarsPlace::Around),
        [
            (Edge::Top, Rect::from_min_max(pos2(1860.0, 30.0), pos2(3840.0, 110.0))),
            (Edge::Bottom, Rect::from_min_max(pos2(1860.0, 1016.0), pos2(3840.0, 1080.0))),
            (Edge::Left, Rect::from_min_max(pos2(1860.0, 110.0), pos2(1920.0, 1016.0))),
        ]
    );
    // margins all round: every bar in its margin, none over the world
    let middle = Rect::from_min_max(pos2(960.0, 135.0), pos2(2880.0, 945.0));
    for (_, strip) in bars_strips(window, middle, BarsPlace::Around, bars) {
        assert!(!strip.intersects(middle.shrink(0.5)), "{strip:?}");
    }
    // a viewer without bars draws none
    assert!(bars_strips(window, world, BarsPlace::Around, Thickness::default()).is_empty());
}

/// The diagram with the bars around the world, drawn with the software
/// painter: the side bar in the left margin, the top bars in the margin
/// above, the bottom ones inside (no margin under the world).
/// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps `diagram-bars-<place>.png`
/// for the three places.
#[test]
fn the_diagram_draws_the_bars_in_the_margins_that_have_room() {
    use crate::folderstorm::viewport::bars::{self, BarsPlace, Thickness};
    let theme = crate::theme::Theme::default();
    let p = theme.skin_palette();
    let (width, height) = (900u32, 360u32);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32));
    let desktop = two();
    for (place, name) in [
        (BarsPlace::Inside, "inside"),
        (BarsPlace::Around, "around"),
        (BarsPlace::Window, "window"),
    ] {
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        bars::set_last_thickness(
            &ctx,
            Thickness {
                left: 120.0,
                right: 0.0,
                top: 90.0,
                bottom: 110.0,
            },
        );
        let mut s = ViewportSettings {
            enabled: true,
            left: 1920,
            top: 100,
            ..Default::default()
        };
        place.set(&mut s);
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut painter = crate::crash::software::Painter::default();
        let mut strips = Vec::new();
        let mut world = None;
        for frame in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(f64::from(frame) / 60.0),
                    ..Default::default()
                },
                |ui| {
                    egui::Frame::new().inner_margin(10.0).show(ui, |ui| {
                        widget::show(ui, &p, &mut s, &desktop);
                    });
                },
            );
            strips.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<(egui::Color32, Rect)>) {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                    egui::Shape::Rect(r) => out.push((r.fill, r.rect)),
                    _ => {}
                }
            }
            let mut rects = Vec::new();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut rects);
            }
            for (fill, rect) in rects {
                if fill == p.violet.gamma_multiply(0.7) {
                    strips.push(rect);
                } else if fill == p.teal.gamma_multiply(0.28) {
                    world = Some(rect);
                }
            }
            let primitives = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            pixels.fill(0);
            painter.paint(
                &mut pixels,
                [width as usize, height as usize],
                &primitives,
                &output.textures_delta,
                output.pixels_per_point,
                p.panel,
            );
            output.textures_delta.clear();
        }
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("diagram-bars-{name}.png"));
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
        let world = world.expect("the world is drawn");
        assert_eq!(strips.len(), 3, "{name}: top, bottom and left bars");
        let outside = |strip: &Rect| !strip.intersects(world.shrink(0.5));
        match place {
            // all of them over the world
            BarsPlace::Inside => assert!(strips.iter().all(|strip| world.expand(0.5).contains_rect(*strip)), "{strips:?}"),
            // the left and top ones in their margins, the bottom one inside
            BarsPlace::Around => {
                assert_eq!(
                    strips.iter().filter(|strip| outside(strip)).count(),
                    2,
                    "{strips:?} around {world:?}"
                );
                assert!(
                    strips.iter().any(|strip| strip.right() <= world.left() + 0.5),
                    "the side bar left of the world"
                );
                assert!(
                    strips.iter().any(|strip| strip.bottom() <= world.top() + 0.5),
                    "the top bars above the world"
                );
            }
            // along the window's own edges: the left one far from the world
            BarsPlace::Window => assert!(strips.iter().any(|strip| strip.right() < world.left() - 20.0), "{strips:?}"),
        }
    }
}
