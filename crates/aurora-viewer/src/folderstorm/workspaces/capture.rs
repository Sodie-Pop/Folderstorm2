//! Reading the arrangement of now: where the allowlisted windows are, the
//! interface they are in, the monitors under the viewer window.
//!
//! A window's place comes from egui's state for its id (position, and size
//! once shown in this session) and from Aurora's two entries for a floater
//! (`ui/widgets.rs`): whether it is minimized, and the size it has when
//! expanded. Nothing is read from the windows' contents or from the world.

use super::monitors;
use super::store::{Frame, MAX_MARGIN, Monitor, Placement, Placements, WindowFrame};
use super::window::Seen;
use super::windows::{self, Kind, Window};
use crate::folderstorm::viewport::ViewportSettings;
use egui::{Pos2, Vec2};

/// An interface smaller than this is not one to place windows in: a
/// minimized viewer window, the first frames before the surface has its
/// size (as `ui/layout.rs` has it for its own records).
const MIN_FRAME: Vec2 = Vec2::new(320.0, 240.0);

/// Aurora's entry for the size a floater has when it is not minimized
/// (`window_id.with("expanded_size")`, persisted with the layout).
const EXPANDED_SIZE: &str = "expanded_size";

/// The interface of now: its frame and where its top-left corner is in
/// egui's coordinates (under the session host's bar when it has one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub frame: Frame,
    pub origin: Pos2,
}

/// The interface of now, when it can hold windows. Not before the first
/// frame: egui then has a made-up screen, not the viewer window.
pub fn room(ctx: &egui::Context) -> Option<Room> {
    let rect = ctx.content_rect();
    let scale = ctx.pixels_per_point();
    let usable = ctx.cumulative_frame_nr() > 0
        && rect.is_finite()
        && rect.width() >= MIN_FRAME.x
        && rect.height() >= MIN_FRAME.y
        && scale.is_finite()
        && scale > 0.0;
    usable.then(|| Room {
        frame: Frame {
            width: rect.width(),
            height: rect.height(),
            pixels_per_point: scale,
        },
        origin: rect.min,
    })
}

fn expanded_size(ctx: &egui::Context, id: egui::Id) -> Option<Vec2> {
    ctx.data_mut(|d| d.get_persisted::<Vec2>(id.with(EXPANDED_SIZE)))
}

fn sized(size: Vec2) -> Option<Vec2> {
    (size.is_finite() && size.x > 0.0 && size.y > 0.0).then_some(size)
}

/// Where `window` is, open or closed: None for a window the viewer has
/// never placed, or whose size is not known yet (closed since the start).
pub fn window(ctx: &egui::Context, window: &Window, origin: Pos2) -> Option<Placement> {
    let id = egui::Id::unique(window.id);
    let state = egui::AreaState::load(ctx, id)?;
    state.pivot_pos?;
    let minimized = window.kind == Kind::Floater && crate::ui::widgets::floater_minimized(ctx, window.id);
    // minimized, the window's own size is its title bar's
    let shown = state.size.and_then(sized).filter(|_| !minimized);
    let size = shown.or_else(|| expanded_size(ctx, id).and_then(sized))?;
    // (Aurora's windows are pivoted at their top-left corner, so this holds
    // for one egui has no size for yet)
    let pos = state.left_top_pos();
    let place = Placement {
        x: pos.x - origin.x,
        y: pos.y - origin.y,
        width: size.x,
        height: size.y,
        minimized,
    };
    place.ok().then_some(place)
}

/// The *Window positions* part of now: every allowlisted window the viewer
/// knows, open or closed. None while the interface cannot hold windows.
pub fn windows(ctx: &egui::Context) -> Option<Placements> {
    let room = room(ctx)?;
    let windows = windows::all()
        .iter()
        .filter_map(|w| Some((w.id.to_owned(), window(ctx, w, room.origin)?)))
        .collect();
    Some(Placements {
        frame: room.frame,
        windows,
    })
}

/// The *World viewport and bars* part of now: the settings, their margins
/// within the store's limit.
pub fn world(settings: &ViewportSettings) -> ViewportSettings {
    ViewportSettings {
        left: settings.left.min(MAX_MARGIN),
        right: settings.right.min(MAX_MARGIN),
        top: settings.top.min(MAX_MARGIN),
        bottom: settings.bottom.min(MAX_MARGIN),
        ..settings.clone()
    }
}

/// The monitors as the app last saw them (`diagram::set_desktop`); none
/// where the system does not tell where the window is (Wayland).
pub fn monitors(ctx: &egui::Context) -> Vec<Monitor> {
    let desktop = crate::folderstorm::diagram::desktop(ctx);
    if desktop.position.is_none() {
        return Vec::new();
    }
    desktop
        .monitors
        .iter()
        .map(|m| Monitor {
            x: m.rect[0],
            y: m.rect[1],
            width: m.rect[2].max(0) as u32,
            height: m.rect[3].max(0) as u32,
            scale: m.scale,
        })
        .filter(Monitor::ok)
        .take(super::store::MAX_MONITORS)
        .collect()
}

/// A borderless window this close to the monitors it covers, in pixels,
/// covers them exactly.
const COVERS: i32 = 2;

/// The *Viewer window* part of now (section 8.1): Aurora's full screen is
/// full screen on its monitor; a window without its border lying exactly
/// over monitors is borderless over them; anything else is a normal
/// window, a maximized one with the place and size Aurora remembers for
/// it. `monitors` is the list its places count in. None for a minimized
/// window: it has no place to read. The lock box is the caller's.
pub fn viewer_window(seen: &Seen, monitors: &[Monitor]) -> Option<WindowFrame> {
    if seen.minimized || seen.inner.0 == 0 || seen.inner.1 == 0 {
        return None;
    }
    let rect = seen.rect();
    let place = |index: usize| u8::try_from(index).ok();
    if seen.fullscreen {
        let on =
            rect.and_then(|[x, y, w, h]| monitors::monitor_at(i64::from(x) + i64::from(w) / 2, i64::from(y) + i64::from(h) / 2, monitors));
        return Some(WindowFrame::Fullscreen {
            monitor: on.and_then(place).unwrap_or(0),
        });
    }
    if !seen.decorated
        && !seen.maximized
        && let Some([x, y, w, h]) = rect
    {
        let inside = |m: &&Monitor| {
            let (mx, my, mw, mh) = (i64::from(m.x), i64::from(m.y), i64::from(m.width), i64::from(m.height));
            let slack = i64::from(COVERS);
            mx >= i64::from(x) - slack
                && my >= i64::from(y) - slack
                && mx + mw <= i64::from(x) + i64::from(w) + slack
                && my + mh <= i64::from(y) + i64::from(h) + slack
        };
        let covered: Vec<&Monitor> = monitors.iter().filter(inside).collect();
        let exact = monitors::cover(covered.iter().copied())
            .is_some_and(|cover| cover.iter().zip([x, y, w, h]).all(|(a, b)| (a - b).abs() <= COVERS));
        let places: Option<Vec<u8>> = monitors
            .iter()
            .enumerate()
            .filter(|(_, m)| inside(m))
            .map(|(index, _)| place(index))
            .collect();
        if exact && let Some(places) = places {
            return Some(WindowFrame::Borderless {
                monitors: places,
                locked: !seen.resizable,
            });
        }
    }
    // maximized: where it goes back to, when Aurora knows
    let [x, y, width, height] = match (seen.maximized, seen.normal) {
        (true, Some(normal)) => normal.map(Some),
        _ => [
            seen.outer.map(|at| at.0),
            seen.outer.map(|at| at.1),
            Some(super::window::to_i32(seen.inner.0)),
            Some(super::window::to_i32(seen.inner.1)),
        ],
    };
    Some(WindowFrame::Normal {
        x: x.filter(|_| y.is_some()),
        y: y.filter(|_| x.is_some()),
        width: width.unwrap_or(1).max(1) as u32,
        height: height.unwrap_or(1).max(1) as u32,
        maximized: seen.maximized,
        locked: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::window::Pretend;

    fn two() -> Vec<Monitor> {
        [0, 1920]
            .into_iter()
            .map(|x| Monitor {
                x,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_window_with_its_border_is_read_as_normal() {
        let seen = Pretend::new(-8, 0, 3840, 1040).seen;
        assert_eq!(
            viewer_window(&seen, &two()),
            Some(WindowFrame::Normal {
                x: Some(-8),
                y: Some(0),
                width: 3840,
                height: 1040,
                maximized: false,
                locked: false,
            })
        );
        // where the system does not tell its place: the size alone
        let mut nowhere = seen;
        nowhere.outer = None;
        assert_eq!(
            viewer_window(&nowhere, &[]),
            Some(WindowFrame::Normal {
                x: None,
                y: None,
                width: 3840,
                height: 1040,
                maximized: false,
                locked: false,
            })
        );
        let mut minimized = seen;
        minimized.minimized = true;
        assert_eq!(viewer_window(&minimized, &two()), None);
    }

    #[test]
    fn a_maximized_window_keeps_the_place_it_goes_back_to() {
        let mut seen = Pretend::new(1912, -8, 1936, 1056).seen;
        seen.maximized = true;
        seen.normal = Some([2100, 120, 1280, 720]);
        assert_eq!(
            viewer_window(&seen, &two()),
            Some(WindowFrame::Normal {
                x: Some(2100),
                y: Some(120),
                width: 1280,
                height: 720,
                maximized: true,
                locked: false,
            })
        );
    }

    #[test]
    fn full_screen_is_read_with_its_monitor() {
        let mut seen = Pretend::new(1920, 0, 1920, 1080).seen;
        seen.fullscreen = true;
        assert_eq!(viewer_window(&seen, &two()), Some(WindowFrame::Fullscreen { monitor: 1 }));
        assert_eq!(viewer_window(&seen, &[]), Some(WindowFrame::Fullscreen { monitor: 0 }));
    }

    #[test]
    fn a_window_without_border_over_monitors_is_read_as_borderless() {
        let mut seen = Pretend::new(0, 0, 3840, 1080).seen;
        seen.decorated = false;
        seen.resizable = false;
        assert_eq!(
            viewer_window(&seen, &two()),
            Some(WindowFrame::Borderless {
                monitors: vec![0, 1],
                locked: true,
            })
        );
        let mut right = Pretend::new(1920, 0, 1920, 1080).seen;
        right.decorated = false;
        assert_eq!(
            viewer_window(&right, &two()),
            Some(WindowFrame::Borderless {
                monitors: vec![1],
                locked: false,
            })
        );
        // without its border but not over monitors: a normal window
        let mut loose = Pretend::new(100, 100, 1280, 720).seen;
        loose.decorated = false;
        assert!(matches!(viewer_window(&loose, &two()), Some(WindowFrame::Normal { .. })));
    }
}
