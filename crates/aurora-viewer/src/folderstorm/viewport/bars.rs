//! Folderstorm: where the bars go with a custom world viewport
//! (folderstorm/plans/workspaces.md, checkpoint 3; FEATURES.md row 1.3).
//!
//! Folderstorm (C++) placed its toolbars, chat bar and top bars around the
//! viewport: "side bars sit outside the viewport when there is room, and
//! just inside it when there is not" (doc/help.md "Wide-screen layout").
//! Here the choice has three values ([`BarsPlace`]): inside the world (row
//! 1.1's default), around it, or over the whole window as in Aurora.
//!
//! Aurora lays all its bars out in one rectangle (the window, or with row
//! 1.1 the world). "Around the world" grows the world on each side by that
//! side's bars, when the margin between the world and the window has room
//! for them ([`around`]). A side without room is not grown, so its bars
//! stay inside. How thick a side's bars are is measured from the last
//! frame ([`Thickness::between`]): the bars' area then, less the room it
//! left for the windows.
//!
//! The grown rectangle is not one area for every bar ([`Areas`]): the top
//! and bottom bars keep the world's width, above and below it, and a side
//! toolbar stands in its margin just outside the world's edge, along the
//! world's own height (so centred on the world, not between the top and
//! bottom bars). Laid out in the one grown rectangle, the top and bottom
//! bars ran over the side toolbar's margin, and the toolbar stood between
//! them (the human's report on PR #48). A side toolbar without room
//! outside (the world at the window's edge, or a margin thinner than the
//! toolbar) stays inside the world at that edge, between the top and
//! bottom bars, as with *Inside the world*.

use super::ViewportSettings;
use crate::ui::toolbar::Bar;
use egui::{Rect, pos2};

/// Where the top, bottom and side bars are laid out while the world has a
/// custom viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarsPlace {
    /// Over the world, inside its rectangle.
    Inside,
    /// In the margins beside the world where they have room, inside it
    /// where they do not.
    Around,
    /// Over the whole window, as without a viewport.
    Window,
}

impl BarsPlace {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Inside, Self::Around, Self::Window];

    /// The choice the settings hold. `bars_around` only counts with
    /// `bars_in_viewport`: a file from before the choice existed reads as
    /// Inside or Window, as it did.
    pub fn of(s: &ViewportSettings) -> Self {
        match (s.bars_in_viewport, s.bars_around) {
            (false, _) => Self::Window,
            (true, false) => Self::Inside,
            (true, true) => Self::Around,
        }
    }

    pub fn set(self, s: &mut ViewportSettings) {
        (s.bars_in_viewport, s.bars_around) = match self {
            Self::Inside => (true, false),
            Self::Around => (true, true),
            Self::Window => (false, false),
        };
    }
}

/// How thick the bars are on each side of their area, in points: what they
/// take from the room of the windows there (the islands with their gaps,
/// the Legacy bars, the favourites' row, a side toolbar).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thickness {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// No side's bars are thicker than this share of their area: more is not
/// bars but a room left over from another area.
const MAX_SHARE: f32 = 0.4;

/// A room this far outside its area is still inside it (rounding).
const SLACK: f32 = 0.5;

impl Thickness {
    /// The bars laid out in `area` that left `room` for the windows. None
    /// when `room` is not a room of `area`: outside it, or leaving a side
    /// more than [`MAX_SHARE`] of it.
    pub fn between(area: Rect, room: Rect) -> Option<Self> {
        if !area.is_finite() || !room.is_finite() || !area.is_positive() || !room.is_positive() {
            return None;
        }
        let sides = Self {
            left: room.left() - area.left(),
            right: area.right() - room.right(),
            top: room.top() - area.top(),
            bottom: area.bottom() - room.bottom(),
        };
        let fits = |side: f32, span: f32| side >= -SLACK && side <= span * MAX_SHARE;
        let ok = fits(sides.left, area.width())
            && fits(sides.right, area.width())
            && fits(sides.top, area.height())
            && fits(sides.bottom, area.height());
        ok.then_some(Self {
            left: sides.left.max(0.0),
            right: sides.right.max(0.0),
            top: sides.top.max(0.0),
            bottom: sides.bottom.max(0.0),
        })
    }
}

/// The bars' area around `world`, in a window `window` (both in points):
/// the world grown on each side by that side's `bars` when the margin
/// between the world and the window is at least as thick as they are. A
/// side whose margin is too thin, or that has no bars, is left where it
/// is: its bars stay inside the world.
pub fn around(world: Rect, window: Rect, bars: Thickness) -> Rect {
    let grow = |margin: f32, bar: f32| if bar > 0.0 && margin + SLACK >= bar { bar } else { 0.0 };
    Rect::from_min_max(
        pos2(
            world.left() - grow(world.left() - window.left(), bars.left),
            world.top() - grow(world.top() - window.top(), bars.top),
        ),
        pos2(
            world.right() + grow(window.right() - world.right(), bars.right),
            world.bottom() + grow(window.bottom() - world.bottom(), bars.bottom),
        ),
    )
}

/// Where the bars of *Around the world* are laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    /// The top and bottom bars: as wide as the world, above and below it
    /// where they have room.
    pub rows: Rect,
    /// The side toolbars: `rows` reaching into the left and right margins
    /// where they have room ([`around`]).
    pub sides: Rect,
}

impl Areas {
    pub fn around(world: Rect, window: Rect, bars: Thickness) -> Self {
        let sides = around(world, window, bars);
        Areas {
            rows: Rect::from_x_y_ranges(world.x_range(), sides.y_range()),
            sides,
        }
    }
}

/// The side toolbars' area of this frame, with the world it is around.
#[derive(Clone, Copy, Debug)]
struct Sides {
    frame: u64,
    world: Rect,
    area: Rect,
}

impl Sides {
    /// The toolbar of that side stands in its margin, outside the world.
    fn outside(&self, bar: Bar) -> bool {
        match bar {
            Bar::Left => self.area.left() < self.world.left() - SLACK,
            Bar::Right => self.area.right() > self.world.right() + SLACK,
            Bar::Bottom => false,
        }
    }
}

fn sides_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_bars_sides")
}

/// Says where the side toolbars go this frame: `around` is the world and
/// the area [`Areas::sides`] gives around it; None when the bars are not
/// around the world (the side toolbars are then laid out with the others).
pub fn set_sides(ctx: &egui::Context, around: Option<(Rect, Rect)>) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| match around {
        Some((world, area)) => d.insert_temp(sides_id(), Sides { frame, world, area }),
        None => {
            d.remove_temp::<Sides>(sides_id());
        }
    });
}

/// What [`set_sides`] said this frame or the last one (the hidden
/// interface's edges are read before the bars are laid out).
fn sides_set(ctx: &egui::Context) -> Option<Sides> {
    let now = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<Sides>(sides_id()))
        .filter(|sides| sides.frame.saturating_add(1) >= now)
}

/// The rectangle the side toolbars are laid out in: the other bars'
/// ([`super::bars_screen`]), reaching into the margins where the side
/// toolbars stand outside the world.
pub fn side_screen(ctx: &egui::Context) -> Rect {
    let rows = super::bars_screen(ctx);
    match sides_set(ctx) {
        Some(sides) => Rect::from_x_y_ranges(sides.area.x_range(), rows.y_range()),
        None => rows,
    }
}

/// The hook of `ui::bars::side::bars`. Returns the side toolbars' screen
/// ([`side_screen`]). Where a toolbar stands outside the world, `room`
/// (the windows' room between the top and bottom bars) is made to reach
/// its margin, so that what the toolbar then takes from it leaves the
/// world's edge; and with Firestorm's bars (not `islands`), whose side
/// toolbars are panels of the `Ui` they are given, a `Ui` over that wider
/// room is returned for them.
pub fn sides(ui: &mut egui::Ui, room: &mut Rect, islands: bool) -> (Rect, Option<egui::Ui>) {
    let screen = side_screen(ui.ctx());
    let Some(sides) = sides_set(ui.ctx()) else {
        return (screen, None);
    };
    let (left, right) = (sides.outside(Bar::Left), sides.outside(Bar::Right));
    if !left && !right {
        return (screen, None);
    }
    if left {
        room.min.x = room.min.x.min(sides.area.left());
    }
    if right {
        room.max.x = room.max.x.max(sides.area.right());
    }
    let panels = (!islands).then(|| {
        let rows = ui.available_rect_before_wrap();
        ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_x_y_ranges(room.x_range(), rows.y_range())))
    });
    (screen, panels)
}

/// The hook of `ui::bars::side`'s island: the screen and the room the
/// toolbar `bar` is placed in. `room` is the windows' room between the top
/// and bottom bars; a toolbar outside the world has nothing to do with
/// them, and stands along the world's own height.
pub fn side(ctx: &egui::Context, bar: Bar, room: Rect) -> (Rect, Rect) {
    let screen = side_screen(ctx);
    match sides_set(ctx).filter(|sides| sides.outside(bar)) {
        Some(sides) => (screen, Rect::from_x_y_ranges(room.x_range(), sides.world.y_range())),
        None => (screen, room),
    }
}

fn thickness_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_bars_thickness")
}

/// How thick the bars were on each side in the last frame: `room` is the
/// room they left for the windows then (Aurora's `Interface::last_room`),
/// measured against the area they were laid out in ([`super::set_bars_area`];
/// the side toolbars' own, [`set_sides`], for the left and right). To be
/// called before this frame's [`set_sides`].
/// When the last frame drew no bars, or `room` is not one of that area (the
/// bars were hidden while the world moved), the last measure stands.
pub fn thickness(ctx: &egui::Context, room: Option<Rect>) -> Thickness {
    let now = ctx.cumulative_frame_nr();
    let area = ctx
        .data(|d| d.get_temp::<super::BarsArea>(super::bars_frame_id()))
        .filter(|area| area.frame.saturating_add(1) >= now)
        .map(|area| match (area.rect, sides_set(ctx)) {
            (Some(rows), Some(sides)) => Rect::from_x_y_ranges(sides.area.x_range(), rows.y_range()),
            (rows, _) => rows.unwrap_or_else(|| ctx.content_rect()),
        });
    let measured = area.zip(room).and_then(|(area, room)| Thickness::between(area, room));
    match measured {
        Some(bars) => {
            ctx.data_mut(|d| d.insert_temp(thickness_id(), bars));
            bars
        }
        None => ctx.data(|d| d.get_temp::<Thickness>(thickness_id())).unwrap_or_default(),
    }
}

/// The last measure [`thickness`] took, for the diagram of the world
/// viewport; None before the bars were ever measured.
pub fn last_thickness(ctx: &egui::Context) -> Option<Thickness> {
    ctx.data(|d| d.get_temp::<Thickness>(thickness_id()))
}

/// As if the bars had been measured this thick.
#[cfg(test)]
pub fn set_last_thickness(ctx: &egui::Context, bars: Thickness) {
    ctx.data_mut(|d| d.insert_temp(thickness_id(), bars));
}

impl Thickness {
    /// The same bars in another unit: points to pixels, pixels to diagram
    /// points.
    pub fn scaled(self, by: f32) -> Self {
        Thickness {
            left: self.left * by,
            right: self.right * by,
            top: self.top * by,
            bottom: self.bottom * by,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    const BARS: Thickness = Thickness {
        left: 56.0,
        right: 0.0,
        top: 80.0,
        bottom: 64.0,
    };

    #[test]
    fn the_choice_reads_the_two_switches_and_writes_them_back() {
        let mut s = ViewportSettings::default();
        assert_eq!(BarsPlace::of(&s), BarsPlace::Inside, "row 1.1's default");
        for place in BarsPlace::ALL {
            place.set(&mut s);
            assert_eq!(BarsPlace::of(&s), place);
        }
        // "around" without the bars in the viewport is the whole window
        s.bars_in_viewport = false;
        s.bars_around = true;
        assert_eq!(BarsPlace::of(&s), BarsPlace::Window);
    }

    #[test]
    fn settings_from_before_the_choice_read_as_they_did() {
        let inside: ViewportSettings =
            serde_json::from_str(r#"{"enabled":true,"left":1920,"bars_in_viewport":true}"#).expect("an older file");
        assert_eq!(BarsPlace::of(&inside), BarsPlace::Inside);
        let window: ViewportSettings =
            serde_json::from_str(r#"{"enabled":true,"left":1920,"bars_in_viewport":false}"#).expect("an older file");
        assert_eq!(BarsPlace::of(&window), BarsPlace::Window);
        let none: ViewportSettings = serde_json::from_str("{}").expect("no field at all");
        assert_eq!(BarsPlace::of(&none), BarsPlace::Inside);
        // and the new choice survives a save
        let mut around = inside;
        BarsPlace::Around.set(&mut around);
        let text = serde_json::to_string(&around).expect("saved");
        let back: ViewportSettings = serde_json::from_str(&text).expect("read back");
        assert_eq!(BarsPlace::of(&back), BarsPlace::Around);
        assert_eq!(back, around);
    }

    #[test]
    fn the_bars_are_as_thick_as_what_they_took_from_their_area() {
        let area = rect(1920.0, 0.0, 1920.0, 1080.0);
        let room = rect(1976.0, 80.0, 1864.0, 936.0);
        assert_eq!(Thickness::between(area, room), Some(BARS));
        // no bars at all: the room is the area
        assert_eq!(Thickness::between(area, area), Some(Thickness::default()));
        // half a point outside is rounding, not another area
        let nudged = rect(1919.6, 0.0, 1920.4, 1080.0);
        assert_eq!(Thickness::between(area, nudged), Some(Thickness::default()));
    }

    #[test]
    fn a_room_from_another_area_measures_nothing() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        // the room the bars left in the world, measured against the window
        let room = rect(1976.0, 80.0, 1864.0, 936.0);
        assert_eq!(Thickness::between(window, room), None, "1976 points is not a toolbar");
        // a room outside its area, and rectangles with nothing in them
        assert_eq!(
            Thickness::between(rect(1920.0, 0.0, 1920.0, 1080.0), rect(0.0, 0.0, 800.0, 600.0)),
            None
        );
        assert_eq!(Thickness::between(Rect::NOTHING, room), None);
        assert_eq!(Thickness::between(window, Rect::NAN), None);
        assert_eq!(Thickness::between(window, rect(10.0, 10.0, 0.0, 100.0)), None);
    }

    #[test]
    fn each_side_goes_outside_when_its_margin_has_room() {
        let window = rect(0.0, 0.0, 3840.0, 1200.0);
        // the world on the right monitor, 100 points down and up
        let world = rect(1920.0, 100.0, 1920.0, 1000.0);
        let area = around(world, window, BARS);
        // left: a 1920-point margin holds the 56-point toolbar; right: no
        // margin and no bar; top and bottom: 100 points hold 80 and 64
        assert_eq!(area, Rect::from_min_max(pos2(1864.0, 20.0), pos2(3840.0, 1164.0)));
        // the room the bars then leave is the world itself
        let room = Rect::from_min_max(
            pos2(area.left() + BARS.left, area.top() + BARS.top),
            pos2(area.right() - BARS.right, area.bottom() - BARS.bottom),
        );
        assert_eq!(room, world);
    }

    #[test]
    fn a_side_without_room_keeps_its_bars_inside() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        // the world as high as the window: no room above or below
        let world = rect(1920.0, 0.0, 1920.0, 1080.0);
        let area = around(world, window, BARS);
        assert_eq!(area, Rect::from_min_max(pos2(1864.0, 0.0), pos2(3840.0, 1080.0)));
        // a margin thinner than its bar: 40 points for the 56-point toolbar
        let tight = rect(40.0, 0.0, 3800.0, 1080.0);
        assert_eq!(around(tight, window, BARS), tight);
        // exactly as thick as the bar is room enough
        let exact = rect(56.0, 0.0, 3784.0, 1080.0);
        assert_eq!(around(exact, window, BARS).left(), 0.0);
        // a world filling the window, and no bars at all: nothing moves
        assert_eq!(around(window, window, BARS), window);
        assert_eq!(around(world, window, Thickness::default()), world);
    }

    #[test]
    fn the_thickness_is_measured_from_the_last_frame_s_bars() {
        let ctx = egui::Context::default();
        let screen = rect(0.0, 0.0, 3840.0, 1080.0);
        let world = rect(1920.0, 0.0, 1920.0, 1080.0);
        let room = rect(1976.0, 80.0, 1864.0, 936.0);
        let run = |set: Option<Option<Rect>>, room: Option<Rect>| {
            let mut bars = Thickness::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    bars = thickness(ui.ctx(), room);
                    if let Some(area) = set {
                        super::super::set_bars_area(ui.ctx(), area);
                    }
                },
            );
            output.textures_delta.clear();
            bars
        };
        assert_eq!(run(Some(Some(world)), Some(room)), Thickness::default(), "no bars drawn before");
        assert_eq!(run(Some(Some(world)), Some(room)), BARS);
        // the bars over the whole window last frame: measured against it
        let over = rect(0.0, 80.0, 3840.0, 936.0);
        run(Some(None), Some(room));
        assert_eq!(
            run(Some(None), Some(over)),
            Thickness {
                top: 80.0,
                bottom: 64.0,
                ..Default::default()
            }
        );
        // a frame without bars (login), then a stale room: the last measure
        run(None, Some(over));
        assert_eq!(run(Some(Some(world)), Some(over)).top, 80.0);
        // a room that is not the area's (the world moved while the bars
        // were hidden) changes nothing either
        assert_eq!(run(Some(Some(world)), Some(rect(0.0, 80.0, 1000.0, 936.0))).top, 80.0);
    }

    #[test]
    fn around_the_world_the_rows_keep_its_width_and_the_sides_reach_the_margins() {
        let window = rect(0.0, 0.0, 3840.0, 1200.0);
        let world = rect(1920.0, 100.0, 1920.0, 1000.0);
        let areas = Areas::around(world, window, BARS);
        // the top and bottom bars above and below the world, as wide as it
        assert_eq!(areas.rows, Rect::from_min_max(pos2(1920.0, 20.0), pos2(3840.0, 1164.0)));
        // the left toolbar's 56 points in the left margin
        assert_eq!(areas.sides, Rect::from_min_max(pos2(1864.0, 20.0), pos2(3840.0, 1164.0)));
        // no room outside: the two areas are the world
        let tight = rect(40.0, 0.0, 3800.0, 1200.0);
        assert_eq!(
            Areas::around(tight, window, BARS),
            Areas {
                rows: tight,
                sides: tight
            }
        );
    }

    /// The real side toolbars (`ui::bars::side::bars`), laid out frame
    /// after frame as `app.rs` does: the areas from the last frame's
    /// measure, top and bottom bars of 80 and 64 points in the rows' area,
    /// then the side toolbars in the room they leave.
    struct Viewer {
        ctx: egui::Context,
        settings: crate::ui::toolbar::ToolbarSettings,
        state: crate::ui::toolbar::ToolbarUi,
        panels: crate::ui::Panels,
        islands: bool,
        last_room: Option<Rect>,
        areas: Option<Areas>,
    }

    const TOP: f32 = 80.0;
    const BOTTOM: f32 = 64.0;

    fn button(command: &str) -> crate::ui::skin::ToolButton {
        crate::ui::skin::ToolButton {
            command: command.into(),
            icon: "test-icon".into(),
            tooltip: command.into(),
        }
    }

    impl Viewer {
        fn new(islands: bool, left: bool, right: bool) -> Self {
            let buttons = |on: bool| if on { vec![button("inventory"), button("people")] } else { Vec::new() };
            let ctx = egui::Context::default();
            ctx.all_styles_mut(|s| s.animation_time = 0.0);
            Viewer {
                ctx,
                settings: crate::ui::toolbar::ToolbarSettings {
                    left: buttons(left),
                    right: if right { vec![button("worldmap")] } else { Vec::new() },
                    ..Default::default()
                },
                state: Default::default(),
                panels: crate::ui::Panels::from_settings(&crate::settings::Settings::default()),
                islands,
                last_room: None,
                areas: None,
            }
        }

        /// One frame. `around`: the bars around the world, else inside it.
        fn frame(&mut self, window: Rect, world: Rect, around: bool) {
            use crate::ui::bars;
            let p = crate::theme::Theme::default().palette();
            let layout = crate::ui::skin::Layout::default();
            let ctx = self.ctx.clone();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(window),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx().clone();
                    // `App::folderstorm_bars_rect`
                    let areas = if around {
                        let areas = Areas::around(world, window, thickness(&ctx, self.last_room));
                        set_sides(&ctx, Some((world, areas.sides)));
                        areas
                    } else {
                        set_sides(&ctx, None);
                        Areas {
                            rows: world,
                            sides: world,
                        }
                    };
                    super::super::set_bars_area(&ctx, Some(areas.rows));
                    self.areas = Some(areas);
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(areas.rows));
                    let room = if self.islands {
                        Rect::from_min_max(
                            pos2(areas.rows.left(), areas.rows.top() + TOP),
                            pos2(areas.rows.right(), areas.rows.bottom() - BOTTOM),
                        )
                    } else {
                        let frame = egui::Frame::NONE;
                        egui::Panel::top("test_top")
                            .frame(frame)
                            .exact_size(TOP)
                            .resizable(false)
                            .show(&mut child, |_| {});
                        egui::Panel::bottom("test_bottom")
                            .frame(frame)
                            .exact_size(BOTTOM)
                            .resizable(false)
                            .show(&mut child, |_| {});
                        child.available_rect_before_wrap()
                    };
                    let favorites = bars::favorites::View::default();
                    let row = bars::Row {
                        unread: 0,
                        latest_unread: None,
                        flash: 0.0,
                        flying: false,
                        seated: false,
                        mouselook: false,
                        ao_active: false,
                        locked: true,
                        favorites: &favorites,
                        bar: Bar::Left,
                        style: Default::default(),
                        editing: false,
                    };
                    let (room, _) = bars::side::bars(
                        &mut child,
                        &p,
                        &crate::ui::icons::Icons::default(),
                        &layout,
                        &mut self.state,
                        &self.settings,
                        &mut self.panels,
                        &mut bars::MicButton::default(),
                        &row,
                        room,
                        self.islands,
                        [true, true],
                    );
                    self.last_room = Some(room);
                },
            );
            output.textures_delta.clear();
        }

        fn frames(&mut self, window: Rect, world: Rect, around: bool) {
            for _ in 0..4 {
                self.frame(window, world, around);
            }
        }

        /// Where the buttons of a side toolbar are: the rectangle around them.
        fn toolbar(&self, commands: &[&str]) -> Rect {
            commands.iter().fold(Rect::NOTHING, |all, command| {
                let button = self.ctx.read_response(crate::ui::bars::tool_button_id(command)).expect("a button drawn");
                all.union(button.rect)
            })
        }

        fn room(&self) -> Rect {
            self.last_room.expect("a frame was run")
        }
    }

    const LEFT: [&str; 2] = ["inventory", "people"];

    /// The human's case on PR #48: the world on the right half of a window
    /// over two monitors, as high as the window, a left toolbar.
    #[test]
    fn a_side_toolbar_stands_just_outside_the_world_centred_on_its_height() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        let world = rect(1920.0, 0.0, 1920.0, 1080.0);
        for islands in [true, false] {
            let mut v = Viewer::new(islands, true, false);
            v.frames(window, world, true);
            let toolbar = v.toolbar(&LEFT);
            let areas = v.areas.expect("laid out");
            // outside the world, against its left edge
            assert!(toolbar.right() <= world.left(), "islands={islands}: {toolbar:?}");
            assert!(toolbar.right() >= world.left() - 20.0, "islands={islands}: {toolbar:?}");
            assert!(toolbar.left() >= areas.sides.left(), "islands={islands}: {toolbar:?} in {areas:?}");
            // the top and bottom bars keep the world's width (no room above
            // or below it here: they are inside)
            assert_eq!(areas.rows, world, "islands={islands}");
            // and the windows' room starts at the world's edge
            assert_eq!(v.room().left(), world.left(), "islands={islands}");
            assert_eq!(v.room().right(), world.right(), "islands={islands}");
            if islands {
                // centred on the world's height, not between the top and
                // bottom bars (which are 80 and 64 points: 8 points higher)
                assert!((toolbar.center().y - world.center().y).abs() <= 0.5, "{toolbar:?}");
            }
            // steady: the next frames lay it out at the same place
            v.frames(window, world, true);
            assert_eq!(v.toolbar(&LEFT), toolbar, "islands={islands}");
        }
    }

    #[test]
    fn a_world_with_margins_all_round_has_each_toolbar_in_its_margin() {
        let window = rect(0.0, 0.0, 3840.0, 1200.0);
        let world = rect(960.0, 100.0, 1920.0, 1000.0);
        for islands in [true, false] {
            let mut v = Viewer::new(islands, true, true);
            v.frames(window, world, true);
            let (left, right) = (v.toolbar(&LEFT), v.toolbar(&["worldmap"]));
            assert!(left.right() <= world.left() && left.right() >= world.left() - 20.0, "{left:?}");
            assert!(right.left() >= world.right() && right.left() <= world.right() + 20.0, "{right:?}");
            let areas = v.areas.expect("laid out");
            // the top and bottom bars above and below the world, as wide
            assert_eq!(
                areas.rows,
                Rect::from_min_max(pos2(960.0, 100.0 - TOP), pos2(2880.0, 1100.0 + BOTTOM)),
                "islands={islands}"
            );
            // the windows' room is the world itself
            assert_eq!(v.room(), world, "islands={islands}");
            if islands {
                for toolbar in [left, right] {
                    assert!((toolbar.center().y - world.center().y).abs() <= 0.5, "{toolbar:?}");
                }
            }
        }
    }

    #[test]
    fn a_toolbar_without_room_outside_stays_inside_the_world_at_its_edge() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        // the world on the right half: a right toolbar has the window's
        // edge behind it; and a left margin of 20 points is too thin
        for (world, islands) in [
            (rect(1920.0, 0.0, 1920.0, 1080.0), true),
            (rect(1920.0, 0.0, 1920.0, 1080.0), false),
            (rect(20.0, 0.0, 3820.0, 1080.0), true),
            (rect(20.0, 0.0, 3820.0, 1080.0), false),
        ] {
            let mut v = Viewer::new(islands, true, true);
            v.frames(window, world, true);
            let right = v.toolbar(&["worldmap"]);
            assert!(right.right() <= world.right() && right.left() >= world.right() - 80.0, "{right:?}");
            // between the top and bottom bars, as inside the world
            assert!(right.top() >= TOP && right.bottom() <= 1080.0 - BOTTOM, "{right:?}");
            assert!(v.room().right() <= right.left(), "the windows keep clear of it");
            let left = v.toolbar(&LEFT);
            if world.left() < 100.0 {
                assert!(left.left() >= world.left() && left.right() <= world.left() + 80.0, "{left:?}");
                assert!(v.room().left() >= left.right());
                // exactly as with the bars inside the world
                let mut inside = Viewer::new(islands, true, true);
                inside.frames(window, world, false);
                assert_eq!(inside.toolbar(&LEFT), left, "islands={islands}");
                assert_eq!(inside.room(), v.room(), "islands={islands}");
            } else {
                assert!(left.right() <= world.left(), "{left:?}");
            }
        }
    }

    #[test]
    fn leaving_around_the_world_brings_the_toolbars_back_inside() {
        let window = rect(0.0, 0.0, 3840.0, 1080.0);
        let world = rect(1920.0, 0.0, 1920.0, 1080.0);
        let mut v = Viewer::new(true, true, false);
        v.frames(window, world, true);
        assert!(v.toolbar(&LEFT).right() <= world.left());
        v.frames(window, world, false);
        let toolbar = v.toolbar(&LEFT);
        assert!(toolbar.left() >= world.left(), "{toolbar:?}");
        assert_eq!(side_screen(&v.ctx), world);
    }
}
