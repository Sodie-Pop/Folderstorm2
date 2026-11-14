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
//! A layout with a *Viewer window* part first puts the viewer window in
//! place ([`viewer`], step by step: the system answers when it likes); the
//! layout's windows wait for it, and are fitted to the interface it gives.
//!
//! What a session remembers beside the settings ([`Session`], in egui's
//! temporary data): the arrangement before the last apply (*Previous*),
//! the windows still to place, the baseline the windows are compared
//! with for the "modified" mark, the viewer window, and the card shown
//! when the monitors change.

use super::capture::{self, Room};
use super::fit::{Fitted, fit};
use super::menu::{self, Request};
use super::monitors;
use super::store::{Layout, Monitor, MonitorPolicy, Placement, Placements, Store, StoreError, WindowFrame};
use super::viewer;
use super::window::{Command, Hold, Seen};
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
/// Seconds a card that asks nothing stays up.
const TOLD_FOR: f64 = 12.0;
/// Frames the interface is given to take the viewer window's new size
/// before the layout's windows are placed in it.
const RESIZED_AFTER: u64 = 2;

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
    /// Viewer window. Left out without a word where there is no viewer
    /// window to read (a capture, a forced size).
    pub window: bool,
    /// *Lock the window size*, with the viewer window.
    pub lock: bool,
    /// World viewport and bars.
    pub world: bool,
    /// Window positions.
    pub windows: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts {
            window: true,
            lock: false,
            world: true,
            windows: true,
        }
    }
}

/// An arrangement kept in memory: what *Previous arrangement* brings back.
#[derive(Clone, Debug, PartialEq)]
struct Arrangement {
    /// The viewer window, the monitors it counts in and whether it was
    /// held: kept only when the apply this undoes moves it.
    window: Option<(WindowFrame, Vec<Monitor>, bool)>,
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
    /// Monitors of the layout that are not there.
    pub missing: usize,
    /// The viewer window could not go where the layout has it: it stays
    /// where it is.
    pub left: bool,
    /// The layout's lock was not given: a monitor is missing.
    pub unlocked: bool,
    /// The layout has a viewer window and there is none to drive here.
    pub no_window: bool,
}

/// The card shown over the world when the monitors changed (section 8.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// "Use the layout `name`?" `unlocked`: the window was also unlocked.
    Offer { name: String, unlocked: bool },
    /// The layout `name` was applied without asking.
    Applied { name: String },
    /// The viewer window was unlocked, and no layout fits the new setup.
    Unlocked,
}

/// What a frame asks of the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// The store changed: the settings are to be saved.
    pub save: bool,
    /// What to ask of the viewer window, in order.
    pub commands: Vec<Command>,
    /// *Layouts…* was chosen: Preferences on the Layouts tab.
    pub preferences: bool,
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
    viewer: viewer::State,
    notice: Option<Notice>,
    /// When the card was put up, in seconds.
    notice_at: f64,
    /// What the menu's view was made from ([`menu::stamp`]).
    menu: u64,
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

/// The viewer window as the app last saw it.
fn seen(ctx: &egui::Context) -> Option<Seen> {
    session(ctx, |s| s.viewer.seen().copied())
}

/// Whether there is a viewer window to save in a layout.
pub fn has_viewer_window(ctx: &egui::Context) -> bool {
    seen(ctx).is_some_and(|seen| !seen.minimized)
}

/// The viewer window of now as a layout would save it, without a lock.
pub fn viewer_window(ctx: &egui::Context) -> Option<WindowFrame> {
    window_now(ctx).map(|(frame, _)| frame)
}

/// The *Viewer window* part of now, with the monitors its places count in.
fn window_now(ctx: &egui::Context) -> Option<(WindowFrame, Vec<Monitor>)> {
    let monitors = capture::monitors(ctx);
    Some((capture::viewer_window(&seen(ctx)?, &monitors)?, monitors))
}

/// The arrangement of now. `window`: with the viewer window, for an apply
/// that moves it.
fn arrangement(ctx: &egui::Context, live: &Live, window: bool) -> Arrangement {
    let held = live.store.window_held;
    let locked = held
        && live
            .store
            .active()
            .and_then(|layout| layout.window.as_ref())
            .is_some_and(WindowFrame::locked);
    Arrangement {
        window: window
            .then(|| window_now(ctx))
            .flatten()
            .map(|(frame, monitors)| (frame.with_lock(locked), monitors, held)),
        world: live.world.clone(),
        windows: capture::windows(ctx),
        active: live.store.active_layout.clone(),
        modified: live.store.modified,
    }
}

/// The steps of an apply: the viewer window's begun when the layout has
/// one (`window`: its frame and the monitors it was saved on), the world
/// now, the windows queued for the frame after the viewer window is in
/// place. Whether the window is then held is in the second value.
fn start(
    ctx: &egui::Context,
    live: &mut Live,
    window: Option<(&WindowFrame, &[Monitor])>,
    world: Option<&ViewportSettings>,
    windows: Option<&Placements>,
) -> (Report, bool) {
    let mut report = Report::default();
    // the viewer window first: what it asks is worked out from the window
    // and the monitors of now
    let asked = window.and_then(|(frame, saved)| match seen(ctx) {
        Some(seen) => Some(monitors::resolve(frame, saved, &capture::monitors(ctx), &seen)),
        None => {
            report.no_window = true;
            None
        }
    });
    if let Some(asked) = &asked {
        report.missing = asked.missing;
        report.left = asked.left;
        report.unlocked = asked.unlocked;
    }
    let plan = asked.and_then(|asked| asked.plan);
    let held = plan.is_some_and(|plan| plan.hold != Hold::FREE);
    if let Some(world) = world {
        *live.world = world.clone();
    }
    // (how many windows do not fit is counted again when they are placed:
    // the viewer window may have another size by then)
    let room = capture::room(ctx).filter(|_| plan.is_none());
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
        // a new apply replaces one still under way
        s.viewer.begin(plan, report.missing == 0);
        s.queued = windows.map(|w| Queued {
            windows: w.clone(),
            frame: now,
        });
        s.settle = Some(now + SETTLE);
        s.baseline = None;
        s.notice = None;
    });
    (report, held)
}

/// Applies the layout at `index` of the list. The arrangement it replaces
/// is kept for [`previous`].
pub fn apply(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<Report, StoreError> {
    let layout: Layout = live.store.layout(index).ok_or(StoreError::Missing)?.clone();
    layout.check().map_err(|_| StoreError::Invalid)?;
    let before = arrangement(ctx, live, layout.window.is_some());
    let window = layout.window.as_ref().map(|frame| (frame, layout.monitors.as_slice()));
    let (mut report, held) = start(ctx, live, window, layout.world.as_ref(), layout.windows.as_ref());
    report.name = Some(layout.name.clone());
    live.store.set_active(Some(layout.name));
    live.store.window_held = held;
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
    let before = arrangement(ctx, live, kept.window.is_some());
    let window = kept.window.as_ref().map(|(frame, monitors, _)| (frame, monitors.as_slice()));
    let (report, held) = start(ctx, live, window, Some(&kept.world), kept.windows.as_ref());
    // the marks of then, if that layout is still in the list
    let active = kept.active.filter(|name| live.store.find(name).is_some());
    let modified = kept.modified && active.is_some();
    // held as it was then, by the layout of then
    let held = held
        && kept.window.as_ref().is_some_and(|(_, _, held)| *held)
        && active
            .as_deref()
            .and_then(|name| live.store.find(name))
            .and_then(|(_, layout)| layout.window.as_ref())
            .is_some_and(WindowFrame::holds);
    live.store.set_active(active);
    live.store.modified = modified;
    live.store.window_held = held;
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
        s.viewer.rebaseline(now);
    });
}

/// *Save as new*: the arrangement of now, its chosen `parts`, under
/// `name`; it becomes the active layout.
pub fn save_new(ctx: &egui::Context, live: &mut Live, name: &str, parts: Parts) -> Result<usize, StoreError> {
    let (window, monitors) = match parts.window.then(|| window_now(ctx)).flatten() {
        // (a borderless window read as locked stays so)
        Some((frame, monitors)) => {
            let lock = parts.lock || frame.locked();
            (Some(frame.with_lock(lock)), monitors)
        }
        None => (None, capture::monitors(ctx)),
    };
    if window.is_none() && !parts.world && !parts.windows {
        return Err(StoreError::Invalid);
    }
    let world = parts.world.then(|| capture::world(live.world));
    let windows = match parts.windows {
        true => Some(capture::windows(ctx).ok_or(StoreError::Invalid)?),
        false => None,
    };
    // the window is where the layout has it: a lock asked holds at once
    let held = window.as_ref().is_some_and(WindowFrame::holds);
    let index = live.store.add(Layout::new(name, world, windows, monitors).with_window(window))?;
    live.store.window_held = held;
    rebaseline(ctx);
    Ok(index)
}

/// *Update*: the layout at `index` takes the arrangement of now, for the
/// parts it has, and becomes the active layout.
pub fn update(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<(), StoreError> {
    let world = capture::world(live.world);
    let old = live.store.layout(index).ok_or(StoreError::Missing)?;
    // its lock box is the layout's own; the lock in force stays as it is
    // for the layout in use, and is the box for another one
    let locked = old.window.as_ref().is_some_and(WindowFrame::locked);
    let was_held = if live.store.is_active(index) {
        live.store.window_held
    } else {
        true
    };
    let (window, monitors) = match old.window.as_ref().and_then(|_| window_now(ctx)) {
        Some((frame, monitors)) => (Some(frame.with_lock(locked)), monitors),
        None => (None, capture::monitors(ctx)),
    };
    live.store.update(index, world, capture::windows(ctx), window, monitors)?;
    live.store.window_held = was_held
        && live
            .store
            .layout(index)
            .and_then(|layout| layout.window.as_ref())
            .is_some_and(WindowFrame::holds);
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
    let before = arrangement(ctx, live, false);
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

/// *Unlock the viewer window*: its border and its resizing come back from
/// the next frame; it keeps its place and size. False when nothing held it.
pub fn unlock(store: &mut Store) -> bool {
    std::mem::take(&mut store.window_held)
}

/// The tab's editor: another *Viewer window* for the layout at `index`.
/// On the layout in use, the lock box alone takes effect at once (a normal
/// window's, and a borderless one's while it is held); anything else marks
/// it modified until it is applied.
pub fn set_window(store: &mut Store, index: usize, frame: WindowFrame) -> Result<(), StoreError> {
    let old = store.layout(index).and_then(|layout| layout.window.clone());
    let lock_only = old.is_some_and(|old| old.with_lock(frame.locked()) == frame);
    let (active, modified, held) = (store.is_active(index), store.modified, store.window_held);
    let normal = matches!(frame, WindowFrame::Normal { .. });
    let locked = frame.locked();
    store.set_window(index, frame)?;
    if active && lock_only && (normal || held) {
        store.modified = modified;
        store.window_held = if normal { locked } else { held };
    }
    Ok(())
}

/// The card to show over the world, if any.
pub fn notice(ctx: &egui::Context) -> Option<Notice> {
    session(ctx, |s| s.notice.clone())
}

/// What the monitors' change asks for, by the user's choice: nothing, a
/// card, or (the second value) a layout to apply at once.
fn on_setup(store: &Store, setup: &[Monitor], unlocked: bool) -> (Option<Notice>, Option<usize>) {
    if store.on_monitor_change == MonitorPolicy::Nothing {
        return (None, None);
    }
    // the layout in use was saved on this setup: nothing to offer
    let fits = store
        .active()
        .is_some_and(|layout| !layout.monitors.is_empty() && monitors::same_setup(&layout.monitors, setup));
    let offer = monitors::matching(store, setup).filter(|_| !fits);
    let name = offer.and_then(|index| store.layout(index)).map(|layout| layout.name.clone());
    match (name, store.on_monitor_change) {
        (Some(name), MonitorPolicy::Apply) => (Some(Notice::Applied { name }), offer),
        (Some(name), _) => (Some(Notice::Offer { name, unlocked }), None),
        (None, _) if unlocked => (Some(Notice::Unlocked), None),
        (None, _) => (None, None),
    }
}

/// Runs what the menu and the card asked since the last frame.
fn run_requests(ctx: &egui::Context, live: &mut Live, outcome: &mut Outcome) {
    for request in menu::take(ctx) {
        match request {
            Request::Apply(name) => {
                if let Some((index, _)) = live.store.find(&name)
                    && apply(ctx, live, index).is_ok()
                {
                    outcome.save = true;
                }
            }
            Request::Previous => outcome.save |= previous(ctx, live),
            Request::Unlock => outcome.save |= unlock(live.store),
            Request::Preferences => outcome.preferences = true,
            // (an apply and a way back take the card away themselves)
            Request::Dismiss => session(ctx, |s| s.notice = None),
        }
    }
}

/// Each frame, before the interface's pass: `seen` is the viewer window
/// now (None: there is none to drive). Runs what the menu and the card
/// asked, carries on the viewer window's steps and keeps its lock, places
/// the windows of an apply once the viewer window is in place, asks again
/// at the start for the windows still waiting from the last session, looks
/// at the monitors, and twice a second looks at the windows for the
/// "modified" mark.
pub fn frame(ctx: &egui::Context, live: &mut Live, seen: Option<Seen>) -> Outcome {
    let mut outcome = Outcome::default();
    run_requests(ctx, live, &mut outcome);
    let now = ctx.cumulative_frame_nr();
    let mut s = session(ctx, std::mem::take);
    let found = s.viewer.frame(ctx, live.store, seen, &mut outcome.commands);
    outcome.save |= found.save;
    let mut apply_now = None;
    let t = ctx.input(|i| i.time);
    if let Some(setup) = &found.setup {
        (s.notice, apply_now) = on_setup(live.store, setup, found.unlocked);
        s.notice_at = t;
    }
    // a card that only tells goes away by itself; one that asks waits
    if matches!(s.notice, Some(Notice::Applied { .. } | Notice::Unlocked)) && t - s.notice_at > TOLD_FOR {
        s.notice = None;
    }
    if found.placed
        && let Some(queued) = &mut s.queued
    {
        // the interface takes the window's new size in the frames after
        queued.frame = now + RESIZED_AFTER;
        s.settle = Some(now + RESIZED_AFTER + SETTLE);
    }
    if let Some(room) = capture::room(ctx) {
        outcome.save |= windows_frame(ctx, live, &mut s, &room);
    }
    let stamp = menu::stamp(live.store, s.previous.is_some());
    if stamp != std::mem::replace(&mut s.menu, stamp) {
        menu::publish(ctx, live.store, s.previous.is_some());
    }
    session(ctx, |slot| *slot = s);
    // (with the session back in its place: an apply reads and writes it)
    if let Some(index) = apply_now
        && apply(ctx, live, index).is_ok()
    {
        outcome.save = true;
        let name = live.store.layout(index).map(|layout| layout.name.clone());
        session(ctx, |s| {
            s.notice = name.map(|name| Notice::Applied { name });
            s.notice_at = t;
        });
    }
    outcome
}

/// The windows' part of a frame, in the interface `room`. True when the
/// store changed.
fn windows_frame(ctx: &egui::Context, live: &mut Live, s: &mut Session, room: &Room) -> bool {
    let now = ctx.cumulative_frame_nr();
    let mut save = false;
    if !s.started {
        s.started = true;
        // places asked of closed windows before the last quit
        if let Some(windows) = live.store.active().and_then(|layout| layout.windows.as_ref()) {
            for id in &live.store.pending {
                if let (Some(window), Some(place)) = (windows::find(id), windows.windows.get(id)) {
                    request(ctx, s, window, room, &fit(place, &windows.frame, &room.frame));
                }
            }
        }
        s.settle = Some(now + SETTLE);
    }
    // (the windows wait for the viewer window: they are fitted to the
    // interface it gives)
    let viewer_busy = s.viewer.busy();
    if let Some(queued) = s.queued.take_if(|queued| queued.frame < now && !viewer_busy) {
        s.targets.clear();
        let mut fitted = 0;
        for (id, place) in &queued.windows.windows {
            if let Some(window) = windows::find(id) {
                let place = fit(place, &queued.windows.frame, &room.frame);
                fitted += usize::from(place.changed);
                request(ctx, s, window, room, &place);
            }
        }
        if let Some(report) = &mut s.report {
            report.fitted = fitted;
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
    if !busy && follow_up(ctx, s) {
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
        let current = capture::windows(ctx);
        if let (Some(layout), Some(baseline), Some(current)) = (&layout, &mut s.baseline, &current) {
            // a window of the layout shown for the first time since the
            // baseline: where Aurora opened it is where it is watched from
            for (id, place) in &current.windows {
                if layout.windows.contains_key(id) && !baseline.windows.contains_key(id) {
                    baseline.windows.insert(id.clone(), *place);
                }
            }
            if !live.store.modified && moved(layout, baseline, current, &live.store.pending) {
                live.store.modified = true;
                save = true;
            }
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
    save
}
