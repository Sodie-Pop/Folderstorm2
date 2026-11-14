//! Applying a layout, coming back to the arrangement before it, and
//! telling when the active layout was changed since
//! (folderstorm/plans/workspaces.md, checkpoint 3 in detail).
//!
//! An apply is two steps. The world part is written to the settings at
//! once and takes effect on the next frame. The windows are placed on the
//! next frame ([`frame`], before the interface's pass), when the bars are
//! drawn where the new settings put them and Aurora knows the room the
//! windows move in: each window of the layout gets a placement request
//! (`place_floater`; our own for the mini-map), fitted to the interface of
//! now ([`fit`]). A closed window keeps its request until it opens; its id
//! waits in `Store::pending` so that a restart asks again.
//!
//! Aurora keeps a window inside the interface with the size it had before
//! the request: a window made smaller and moved towards an edge at once
//! stops short of its place. So a window that has taken its request and is
//! not where it was asked is asked for its position again, with its new
//! size ([`Target`]).
//!
//! What a session remembers beside the settings ([`Session`], in egui's
//! temporary data): the arrangement before the last apply (*Previous*),
//! the windows still to place, and the baseline the windows are compared
//! with for the "modified" mark.

use super::capture::{self, Room};
use super::fit::{Fitted, fit};
use super::store::{Layout, Placement, Placements, Store, StoreError};
use super::windows::{self, Kind, Window};
use crate::folderstorm::viewport::ViewportSettings;
use std::collections::{BTreeMap, BTreeSet};

/// Frames an apply, a start or a change of the interface's size is given
/// to settle before the windows are looked at again.
const SETTLE: u64 = 12;
/// Frames between two looks at the windows (half a second at 60 images a
/// second).
const CHECK_EVERY: u64 = 30;
/// A window moved or resized by more than this, in points, is modified.
const MOVED: f32 = 2.0;
/// A window this close to where it was asked, in points, is there.
const ARRIVED: f32 = 1.0;
/// How many times a window is asked for its position again.
const RETRIES: u8 = 2;
/// Frames a window is given to take a request before it is looked at.
const TAKEN_AFTER: u64 = 2;

/// Aurora's entry for a floater's placement request (`ui/widgets.rs`,
/// `PLACE_POS`): there until the floater is next shown.
const PLACE_POS: &str = "place_pos";

/// What a layout changes, both in the viewer's settings: the store and the
/// world viewport.
pub struct Live<'a> {
    pub store: &'a mut Store,
    pub world: &'a mut ViewportSettings,
}

impl<'a> Live<'a> {
    pub fn of(settings: &'a mut crate::settings::Settings) -> Self {
        Live {
            store: &mut settings.folderstorm_workspaces,
            world: &mut settings.folderstorm_viewport,
        }
    }
}

/// The parts of a layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// World viewport and bars.
    pub world: bool,
    /// Window positions.
    pub windows: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts {
            world: true,
            windows: true,
        }
    }
}

/// An arrangement kept in memory: what *Previous arrangement* brings back.
#[derive(Clone, Debug, PartialEq)]
struct Arrangement {
    world: ViewportSettings,
    windows: Option<Placements>,
    active: Option<String>,
    modified: bool,
}

/// What an apply did, for one line under the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The layout applied; None for *Previous arrangement*.
    pub name: Option<String>,
    /// Windows of the layout that were given a place.
    pub windows: usize,
    /// Of those, the ones whose saved place did not fit as it was.
    pub fitted: usize,
    /// Windows of the layout this version does not know: left alone.
    pub unknown: usize,
}

#[derive(Clone, Debug)]
struct Queued {
    windows: Placements,
    /// The frame the apply was asked in: placed in a later one.
    frame: u64,
}

/// Where a window was asked to go, until it is there.
#[derive(Clone, Copy, Debug)]
struct Target {
    /// In egui's coordinates.
    pos: egui::Pos2,
    size: egui::Vec2,
    /// The frame of the last request, and how many were made after the first.
    asked: u64,
    retries: u8,
}

#[derive(Clone, Debug, Default)]
struct Session {
    started: bool,
    previous: Option<Arrangement>,
    queued: Option<Queued>,
    /// Windows given a request and not yet seen at their place.
    targets: BTreeMap<String, Target>,
    /// The frame from which the windows have settled.
    settle: Option<u64>,
    /// Where Aurora put the windows once settled.
    baseline: Option<Placements>,
    /// The frame of the last look at the windows.
    checked: u64,
    report: Option<Report>,
}

fn session_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_session")
}

/// Reads or changes the session's state. `change` must not call into egui.
fn session<R>(ctx: &egui::Context, change: impl FnOnce(&mut Session) -> R) -> R {
    ctx.data_mut(|d| change(d.get_temp_mut_or_default::<Session>(session_id())))
}

/// A place asked of a window without a title bar: its corner and size.
type BareRequest = (egui::Pos2, egui::Vec2);

fn bare_id(window: egui::Id) -> egui::Id {
    window.with("folderstorm_place")
}

/// The place a layout asked for the window without a title bar `window`,
/// once: the hook of `ui::widgets::bare_window`, which then shows it there
/// at that size, as `Floater::show` does with `place_floater`'s request.
pub fn take_bare(ctx: &egui::Context, window: egui::Id) -> Option<egui::Rect> {
    ctx.data_mut(|d| d.remove_temp::<BareRequest>(bare_id(window)))
        .map(|(pos, size)| egui::Rect::from_min_size(pos, size))
}

fn request_bare(ctx: &egui::Context, window: &Window, pos: egui::Pos2, size: egui::Vec2) {
    let id = egui::Id::unique(window.id);
    // as `place_floater`: the place becomes the window's anchor
    crate::ui::layout::reanchor(ctx, id);
    ctx.data_mut(|d| d.insert_temp::<BareRequest>(bare_id(id), (pos, size)));
}

/// Asks `window` to take the place `fitted` in `room` when it is next
/// shown, and remembers where that is.
fn request(ctx: &egui::Context, s: &mut Session, window: &Window, room: &Room, fitted: &Fitted) {
    let pos = room.origin + fitted.pos.to_vec2();
    match window.kind {
        Kind::Floater => {
            crate::ui::widgets::place_floater(ctx, window.id, pos, fitted.size);
            if fitted.minimized {
                crate::ui::widgets::minimize_floater(ctx, window.id);
            }
        }
        Kind::Bare => request_bare(ctx, window, pos, fitted.size),
    }
    s.targets.insert(
        window.id.to_owned(),
        Target {
            pos,
            size: fitted.size,
            asked: ctx.cumulative_frame_nr(),
            retries: 0,
        },
    );
}

/// Looks at the windows asked to move: one that took its request and
/// stopped short of its place is asked for its position again. True when a
/// request was made.
fn follow_up(ctx: &egui::Context, s: &mut Session) -> bool {
    let now = ctx.cumulative_frame_nr();
    let mut asked = false;
    s.targets.retain(|id, target| {
        let Some(window) = windows::find(id) else { return false };
        // closed, or just asked: not laid out with the request yet
        if waiting(ctx, window) || now < target.asked.saturating_add(TAKEN_AFTER) {
            return true;
        }
        let here = egui::AreaState::load(ctx, egui::Id::unique(window.id)).map(|state| state.left_top_pos());
        let arrived = here.is_some_and(|here| (here - target.pos).abs().max_elem() <= ARRIVED);
        if arrived || target.retries >= RETRIES {
            return false;
        }
        match window.kind {
            Kind::Floater => crate::ui::widgets::move_floater(ctx, window.id, target.pos),
            Kind::Bare => request_bare(ctx, window, target.pos, target.size),
        }
        target.asked = now;
        target.retries += 1;
        asked = true;
        true
    });
    asked
}

/// `window` was asked to take a place and has not been shown since: it is
/// closed.
fn waiting(ctx: &egui::Context, window: &Window) -> bool {
    let id = egui::Id::unique(window.id);
    match window.kind {
        Kind::Floater => ctx.data(|d| d.get_temp::<egui::Pos2>(id.with(PLACE_POS)).is_some()),
        Kind::Bare => ctx.data(|d| d.get_temp::<BareRequest>(bare_id(id)).is_some()),
    }
}

fn arrangement(ctx: &egui::Context, live: &Live) -> Arrangement {
    Arrangement {
        world: live.world.clone(),
        windows: capture::windows(ctx),
        active: live.store.active_layout.clone(),
        modified: live.store.modified,
    }
}

/// The two steps of an apply: the world now, the windows queued for the
/// next frame.
fn start(ctx: &egui::Context, live: &mut Live, world: Option<&ViewportSettings>, windows: Option<&Placements>) -> Report {
    if let Some(world) = world {
        *live.world = world.clone();
    }
    let room = capture::room(ctx);
    let mut report = Report::default();
    for (id, place) in windows.iter().flat_map(|w| &w.windows) {
        if windows::find(id).is_none() {
            report.unknown += 1;
            continue;
        }
        report.windows += 1;
        let saved = windows.map(|w| w.frame);
        if let (Some(room), Some(saved)) = (room, saved)
            && fit(place, &saved, &room.frame).changed
        {
            report.fitted += 1;
        }
    }
    let now = ctx.cumulative_frame_nr();
    session(ctx, |s| {
        // a new apply replaces one still waiting for its frame
        s.queued = windows.map(|w| Queued {
            windows: w.clone(),
            frame: now,
        });
        s.settle = Some(now + SETTLE);
        s.baseline = None;
    });
    report
}

/// Applies the layout at `index` of the list. The arrangement it replaces
/// is kept for [`previous`].
pub fn apply(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<Report, StoreError> {
    let layout: Layout = live.store.layout(index).ok_or(StoreError::Missing)?.clone();
    layout.check().map_err(|_| StoreError::Invalid)?;
    let before = arrangement(ctx, live);
    let mut report = start(ctx, live, layout.world.as_ref(), layout.windows.as_ref());
    report.name = Some(layout.name.clone());
    live.store.set_active(Some(layout.name));
    session(ctx, |s| {
        s.previous = Some(before);
        s.report = Some(report.clone());
    });
    Ok(report)
}

/// *Previous arrangement*: swaps the arrangement of now with the one kept
/// by the last apply, so that asking again comes back. False when nothing
/// is kept (one level, for the session only).
pub fn previous(ctx: &egui::Context, live: &mut Live) -> bool {
    let Some(kept) = session(ctx, |s| s.previous.take()) else {
        return false;
    };
    let before = arrangement(ctx, live);
    let report = start(ctx, live, Some(&kept.world), kept.windows.as_ref());
    // the marks of then, if that layout is still in the list
    let active = kept.active.filter(|name| live.store.find(name).is_some());
    let modified = kept.modified && active.is_some();
    live.store.set_active(active);
    live.store.modified = modified;
    session(ctx, |s| {
        s.previous = Some(before);
        s.report = Some(report);
    });
    true
}

/// Whether [`previous`] has an arrangement to bring back.
pub fn has_previous(ctx: &egui::Context) -> bool {
    session(ctx, |s| s.previous.is_some())
}

/// What the last apply did.
pub fn report(ctx: &egui::Context) -> Option<Report> {
    session(ctx, |s| s.report.clone())
}

/// The windows are as the active layout has them from now on: compared
/// with a new baseline once settled.
fn rebaseline(ctx: &egui::Context) {
    let now = ctx.cumulative_frame_nr();
    session(ctx, |s| {
        s.baseline = None;
        s.settle = Some(now + SETTLE);
        s.report = None;
    });
}

/// *Save as new*: the arrangement of now, its chosen `parts`, under
/// `name`; it becomes the active layout.
pub fn save_new(ctx: &egui::Context, live: &mut Live, name: &str, parts: Parts) -> Result<usize, StoreError> {
    if !parts.world && !parts.windows {
        return Err(StoreError::Invalid);
    }
    let world = parts.world.then(|| capture::world(live.world));
    let windows = match parts.windows {
        true => Some(capture::windows(ctx).ok_or(StoreError::Invalid)?),
        false => None,
    };
    let index = live.store.add(Layout::new(name, world, windows, capture::monitors(ctx)))?;
    rebaseline(ctx);
    Ok(index)
}

/// *Update*: the layout at `index` takes the arrangement of now, for the
/// parts it has, and becomes the active layout.
pub fn update(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<(), StoreError> {
    let world = capture::world(live.world);
    live.store.update(index, world, capture::windows(ctx), capture::monitors(ctx))?;
    rebaseline(ctx);
    Ok(())
}

/// *Reset bars and world*: the viewport off and the bars inside, as
/// Folderstorm2 starts; the windows stay where they are. The arrangement
/// before is kept for [`previous`].
pub fn reset_world(ctx: &egui::Context, live: &mut Live) -> bool {
    let fresh = ViewportSettings::default();
    if *live.world == fresh {
        return false;
    }
    let before = arrangement(ctx, live);
    *live.world = fresh;
    session(ctx, |s| {
        s.previous = Some(before);
        s.report = None;
    });
    true
}

/// Whether the active layout no longer describes the arrangement: its
/// world part differs from the settings, or one of its windows was moved
/// since it was applied ([`frame`] sets `Store::modified`).
pub fn modified(store: &Store, world: &ViewportSettings) -> bool {
    store
        .active()
        .is_some_and(|layout| store.modified || layout.world.as_ref().is_some_and(|saved| saved != world))
}

fn differs(a: &Placement, b: &Placement) -> bool {
    a.minimized != b.minimized
        || [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
            .iter()
            .any(|d| d.abs() > MOVED)
}

/// Whether a window of `layout` stands elsewhere `now` than in `baseline`.
/// Windows in `skip` (closed, waiting for their place) and windows one of
/// the two does not know are not compared.
fn moved(layout: &Placements, baseline: &Placements, now: &Placements, skip: &BTreeSet<String>) -> bool {
    layout
        .windows
        .keys()
        .filter(|id| !skip.contains(*id))
        .any(|id| matches!((baseline.windows.get(id), now.windows.get(id)), (Some(a), Some(b)) if differs(a, b)))
}

/// Each frame, before the interface's pass: places the windows of an
/// apply asked in an earlier frame, asks again at the start for the
/// windows still waiting from the last session, and twice a second looks
/// at the windows for the "modified" mark. True when the store changed and
/// the settings are to be saved.
pub fn frame(ctx: &egui::Context, live: &mut Live) -> bool {
    let Some(room) = capture::room(ctx) else {
        return false;
    };
    let now = ctx.cumulative_frame_nr();
    let mut s = session(ctx, std::mem::take);
    let mut save = false;
    if !s.started {
        s.started = true;
        // places asked of closed windows before the last quit
        if let Some(windows) = live.store.active().and_then(|layout| layout.windows.as_ref()) {
            for id in &live.store.pending {
                if let (Some(window), Some(place)) = (windows::find(id), windows.windows.get(id)) {
                    request(ctx, &mut s, window, &room, &fit(place, &windows.frame, &room.frame));
                }
            }
        }
        s.settle = Some(now + SETTLE);
    }
    if let Some(queued) = s.queued.take_if(|queued| queued.frame < now) {
        s.targets.clear();
        for (id, place) in &queued.windows.windows {
            if let Some(window) = windows::find(id) {
                request(ctx, &mut s, window, &room, &fit(place, &queued.windows.frame, &room.frame));
            }
        }
        s.settle = Some(now + SETTLE);
        s.baseline = None;
    }
    // the interface changed size: what Aurora then does with the windows
    // is not the user moving them
    if s.baseline.as_ref().is_some_and(|baseline| !baseline.frame.same(&room.frame)) {
        s.baseline = None;
        s.settle = Some(now + SETTLE);
    }
    let busy = s.queued.is_some() || ctx.input(|i| i.pointer.any_down());
    if !busy && follow_up(ctx, &mut s) {
        s.baseline = None;
        s.settle = Some(now + SETTLE);
    }
    let still_waiting = |ids: &BTreeSet<String>| -> BTreeSet<String> {
        ids.iter()
            .filter(|id| windows::find(id).is_some_and(|window| waiting(ctx, window)))
            .cloned()
            .collect()
    };
    if let Some(at) = s.settle {
        if now >= at && !busy {
            s.settle = None;
            // what was asked and not taken belongs to closed windows
            let asked: BTreeSet<String> = s.targets.keys().chain(&live.store.pending).cloned().collect();
            let pending = if live.store.active_layout.is_some() {
                still_waiting(&asked)
            } else {
                BTreeSet::new()
            };
            if pending != live.store.pending {
                live.store.pending = pending;
                save = true;
            }
            s.baseline = capture::windows(ctx);
            s.checked = now;
        }
    } else if !busy && now >= s.checked.saturating_add(CHECK_EVERY) {
        s.checked = now;
        let layout = live.store.active().and_then(|layout| layout.windows.clone());
        if !live.store.modified
            && let (Some(layout), Some(baseline)) = (&layout, &s.baseline)
            && let Some(current) = capture::windows(ctx)
            && moved(layout, baseline, &current, &live.store.pending)
        {
            live.store.modified = true;
            save = true;
        }
        // a window that waited was opened and took its place: Aurora's
        // doing, so the baseline is taken again
        let pending = still_waiting(&live.store.pending);
        if pending != live.store.pending {
            live.store.pending = pending;
            save = true;
            s.baseline = None;
            s.settle = Some(now + SETTLE);
        }
    }
    session(ctx, |slot| *slot = s);
    save
}
