//! Reading the arrangement of now: where the allowlisted windows are, the
//! interface they are in, the monitors under the viewer window.
//!
//! A window's place comes from egui's state for its id (position, and size
//! once shown in this session) and from Aurora's two entries for a floater
//! (`ui/widgets.rs`): whether it is minimized, and the size it has when
//! expanded. Nothing is read from the windows' contents or from the world.

use super::store::{Frame, MAX_MARGIN, Monitor, Placement, Placements};
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
