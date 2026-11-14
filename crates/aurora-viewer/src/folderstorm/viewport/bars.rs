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
//! 1.1 the world). "Around the world" keeps that: the rectangle is the
//! world grown on each side by that side's bars, when the margin between
//! the world and the window has room for them ([`around`]). A side without
//! room is not grown, so its bars stay inside. How thick a side's bars are
//! is measured from the last frame ([`Thickness::between`]): the bars'
//! area then, less the room it left for the windows.

use super::ViewportSettings;
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

fn thickness_id() -> egui::Id {
    egui::Id::unique("folderstorm_viewport_bars_thickness")
}

/// How thick the bars were on each side in the last frame: `room` is the
/// room they left for the windows then (Aurora's `Interface::last_room`),
/// measured against the area they were laid out in ([`super::set_bars_area`]).
/// When the last frame drew no bars, or `room` is not one of that area (the
/// bars were hidden while the world moved), the last measure stands.
pub fn thickness(ctx: &egui::Context, room: Option<Rect>) -> Thickness {
    let now = ctx.cumulative_frame_nr();
    let area = ctx
        .data(|d| d.get_temp::<super::BarsArea>(super::bars_frame_id()))
        .filter(|area| area.frame.saturating_add(1) >= now)
        .map(|area| area.rect.unwrap_or_else(|| ctx.content_rect()));
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
}
