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
//! now ([`fit`]). A window the layout was saved with open and that is
//! closed now is opened first, the app's own way ([`Shell`]); one that
//! cannot be opened now is left closed and counted in the report. Any
//! other closed window keeps its request until it opens; its id waits in
//! `Store::pending` so that a restart asks again. A layout never closes a
//! window.
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
use super::context::{self, Hud};
use super::fit::{Fitted, fit};
use super::menu::{self, Request};
use super::monitors;
use super::pick;
use super::store::{Layout, Monitor, MonitorPolicy, Placement, Placements, Store, StoreError, WindowFrame};
use super::switcher;
use super::viewer;
use super::window::{Command, Hold, Seen};
use super::windows::{self, Kind, Window};
use super::workspace::{self, Folders, Workspace};
use crate::folderstorm::viewport::ViewportSettings;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

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
/// Every this many frames, the HUDs the workspaces put on are looked at
/// ([`context::Ours::observe`]).
const HUDS_EVERY: u64 = 30;
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

/// The viewer's windows as the app has them: which are open, and the app's
/// own way of opening one (the path of its menu entry or toolbar button).
/// A layout opens the windows it was saved with open, and never closes one.
pub trait Shell {
    fn is_open(&self, window: &Window) -> bool;
    /// Opens `window`, which is closed. False when it cannot be opened now
    /// (RLVa hides it, or the viewer's settings no longer have that
    /// window): it is left closed and the apply says so.
    fn open(&mut self, ctx: &egui::Context, window: &Window) -> bool;
    /// Closes `window`, which is open: a workspace's business (never a
    /// layout's).
    fn close(&mut self, ctx: &egui::Context, window: &Window);

    // Inventory folder windows (*Remember Inventory folders*). A viewer
    // without them (a test of the places alone) has no account and none.

    /// The account logged in: folder ids mean something for it only.
    fn account(&self) -> Option<Uuid> {
        None
    }
    /// The open folder windows: each floater's id and its folder.
    fn folder_windows(&self) -> Vec<(String, Uuid)> {
        Vec::new()
    }
    /// Whether the Inventory has the folder now (not deleted, and loaded).
    fn has_folder(&self, _folder: Uuid) -> bool {
        false
    }
    /// Opens a window on `folder`: its floater's id, to place it. None
    /// when it cannot be opened now (RLVa hides the Inventory).
    fn open_folder(&mut self, _ctx: &egui::Context, _folder: Uuid) -> Option<String> {
        None
    }
    /// Closes the folder window of that floater's id.
    fn close_folder(&mut self, _window: &str) {}

    // A workspace's HUDs (row 1.7): what Preferences shows of them.

    /// The HUDs worn now, each with its point (none when nobody needs
    /// them: Preferences is closed).
    fn worn_huds(&self) -> Vec<Hud> {
        Vec::new()
    }
    /// The name of the user's own Inventory item `item`, if it has it.
    fn item_name(&self, _item: Uuid) -> Option<String> {
        None
    }
    /// The items the outfit has on now: worn, or linked in the Current
    /// Outfit (a HUD stays linked through a teleport). None while it is
    /// not known (the Current Outfit not loaded). Asked only while the
    /// workspaces look after HUDs they put on ([`context::Ours`]).
    fn huds_on(&self) -> Option<BTreeSet<Uuid>> {
        None
    }
}

/// A viewer that opens nothing and says every window is closed: the tests
/// of the places alone.
#[cfg(test)]
pub struct NoShell;

#[cfg(test)]
impl Shell for NoShell {
    fn is_open(&self, _: &Window) -> bool {
        false
    }

    fn open(&mut self, _: &egui::Context, _: &Window) -> bool {
        false
    }

    fn close(&mut self, _: &egui::Context, _: &Window) {}
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
    /// The windows open then and the workspace in use then: kept only when
    /// the apply this undoes is a workspace's (it opens and closes); and
    /// the folder windows of then, when that workspace has its own.
    shown: Option<(BTreeSet<String>, Option<String>, Option<Folders>)>,
    /// The graphics and the camera of then, when the apply this undoes
    /// changed them (row 1.7): said by the app once it has.
    context: Option<context::Before>,
}

/// What an apply did, for one line under the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The layout applied; None for *Previous arrangement*, and for a
    /// workspace that keeps the layout in use.
    pub name: Option<String>,
    /// The workspace applied, if one was.
    pub workspace: Option<String>,
    /// Open windows the workspace does not list: closed.
    pub closed: usize,
    /// The workspace names a layout that cannot be used: the layout in use
    /// was kept.
    pub no_layout: bool,
    /// Inventory folders of the workspace that are not in the Inventory
    /// now (deleted, or not loaded yet): skipped.
    pub folders_missing: usize,
    /// Folder windows not opened: RLVa hides the Inventory.
    pub folders_blocked: usize,
    /// The workspace's folders were saved by another account: skipped.
    pub folders_other_account: bool,
    /// Windows of the layout that were given a place.
    pub windows: usize,
    /// Of those, the ones whose saved place did not fit as it was.
    pub fitted: usize,
    /// Windows of the layout this version does not know: left alone.
    pub unknown: usize,
    /// Closed windows the layout was saved with open, or that the
    /// workspace lists: opened.
    pub opened: usize,
    /// Of the same, the ones that could not be opened now: left closed.
    pub blocked: usize,
    /// Monitors of the layout that are not there.
    pub missing: usize,
    /// The viewer window could not go where the layout has it: it stays
    /// where it is.
    pub left: bool,
    /// The layout's lock was not given: a monitor is missing.
    pub unlocked: bool,
    /// The layout has a viewer window and there is none to drive here.
    pub no_window: bool,
    /// What the app did with the workspace's context (row 1.7), once it
    /// has said.
    pub context: Option<context::Told>,
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
    /// A workspace's context to apply, or the graphics and the camera to
    /// put back (row 1.7): the app does it, then says what it did
    /// ([`context_done`]).
    pub context: Option<context::Ask>,
}

#[derive(Clone, Debug)]
struct Queued {
    windows: Placements,
    /// The frame the apply was asked in: placed in a later one.
    frame: u64,
}

/// The windows a workspace has open, until they are opened and the others
/// closed: with its layout's windows when it has one, in the next frame
/// otherwise.
#[derive(Clone, Debug)]
struct QueuedOpen {
    open: BTreeSet<String>,
    /// Its Inventory folder windows, when it remembers them.
    folders: Option<Folders>,
    /// The frame the apply was asked in: done in a later one.
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
    queued_open: Option<QueuedOpen>,
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
    /// Why the switcher's last *Save new* or *Update* was refused.
    refused: Option<StoreError>,
    /// What the app is to do with a workspace's context in the next frame.
    context: Option<context::Ask>,
    /// The HUDs the workspaces put on for the account logged in (row 1.7):
    /// the only ones a switch takes off.
    ours: Option<context::Ours>,
    /// The context handed last may be undone by *Previous* for its HUDs:
    /// a switch with *Take off the previous workspace's HUDs*, or a
    /// *Previous* (so that pressing it again comes back).
    undo_huds: bool,
    /// The card is not drawn: the interface is hidden, or in mouselook
    /// ([`set_notice_hidden`]).
    notice_hidden: bool,
}

fn session_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_session")
}

/// Reads or changes the session's state. `change` must not call into egui.
fn session<R>(ctx: &egui::Context, change: impl FnOnce(&mut Session) -> R) -> R {
    ctx.data_mut(|d| change(d.get_temp_mut_or_default::<Session>(session_id())))
}

/// A place asked of a window without a title bar: its corner and size,
/// and how it was given so far.
#[derive(Clone, Copy, Debug)]
struct BareRequest {
    pos: egui::Pos2,
    size: egui::Vec2,
    /// The window has been shown with it once at least.
    taken: bool,
    /// The frames it was given to the window egui knew the size of, and
    /// the last one.
    sized_frames: u8,
    last_frame: u64,
}

/// How many frames a request is given to a window egui knows the size of.
/// A window egui has no size for (closed since the start: egui keeps no
/// size from one session to the next) is only measured on its first frame,
/// and Aurora's `layout::Slot` then puts it back where it was recorded;
/// and egui keeps a window inside the screen with the size it had the
/// frame before. So the request holds until the window has been shown
/// twice at its size: the second time it is where it was asked, at that
/// size, and recorded there (the human's report: the mini-map opened by a
/// layout stayed where it was before).
const BARE_HOLD: u8 = 2;

fn bare_id(window: egui::Id) -> egui::Id {
    window.with("folderstorm_place")
}

/// The place a layout asked for the window without a title bar `window`:
/// the hook of `ui::widgets::bare_window`, before it is shown, which then
/// shows it there at that size, as `Floater::show` does with
/// `place_floater`'s request. Given until the window has been shown at its
/// size ([`BARE_HOLD`]). Also keeps the window's size for a layout saved
/// while it is closed ([`capture::remember_bare_size`]).
pub fn take_bare(ctx: &egui::Context, window: egui::Id) -> Option<egui::Rect> {
    capture::remember_bare_size(ctx, window);
    let sized = egui::AreaState::load(ctx, window).is_some_and(|state| state.size.is_some());
    let now = ctx.cumulative_frame_nr();
    let id = bare_id(window);
    ctx.data_mut(|d| {
        let mut request = d.get_temp::<BareRequest>(id)?;
        request.taken = true;
        // (once a frame: egui may lay a frame out twice)
        if sized && (request.sized_frames == 0 || request.last_frame != now) {
            request.sized_frames = request.sized_frames.saturating_add(1);
            request.last_frame = now;
        }
        if request.sized_frames >= BARE_HOLD {
            d.remove::<BareRequest>(id);
        } else {
            d.insert_temp(id, request);
        }
        Some(egui::Rect::from_min_size(request.pos, request.size))
    })
}

fn request_bare(ctx: &egui::Context, window: &Window, pos: egui::Pos2, size: egui::Vec2) {
    let id = egui::Id::unique(window.id);
    // as `place_floater`: the place becomes the window's anchor
    crate::ui::layout::reanchor(ctx, id);
    let request = BareRequest {
        pos,
        size,
        taken: false,
        sized_frames: 0,
        last_frame: 0,
    };
    ctx.data_mut(|d| d.insert_temp(bare_id(id), request));
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
        Kind::Bare => ctx
            .data(|d| d.get_temp::<BareRequest>(bare_id(id)))
            .is_some_and(|request| !request.taken),
    }
}

/// The viewer window as the app last saw it.
pub(super) fn seen(ctx: &egui::Context) -> Option<Seen> {
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
        shown: None,
        context: None,
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
        s.queued_open = None;
        s.context = None;
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
    let mut before = arrangement(ctx, live, kept.window.is_some());
    // a workspace's apply is undone with its windows, and comes back with them
    before.shown = kept.shown.as_ref().map(|(_, _, folders)| {
        let folders = folders.as_ref().and_then(|_| capture::folders(ctx));
        (capture::open(ctx), live.store.active_workspace.clone(), folders)
    });
    let window = kept.window.as_ref().map(|(frame, monitors, _)| (frame, monitors.as_slice()));
    let (report, held) = start(ctx, live, window, Some(&kept.world), kept.windows.as_ref());
    if let Some((open, workspace, folders)) = kept.shown {
        let frame = ctx.cumulative_frame_nr();
        session(ctx, |s| s.queued_open = Some(QueuedOpen { open, folders, frame }));
        live.store.active_workspace = workspace.filter(|name| live.store.find_workspace(name).is_some());
    }
    // the graphics, the camera and the HUDs of then: the HUDs the switch
    // put on are taken off only while the workspaces still look after them
    if let Some(mut before) = kept.context {
        session(ctx, |s| {
            if let Some(huds) = &mut before.huds {
                let ours = s.ours.as_ref().filter(|ours| ours.account == huds.account);
                huds.added.retain(|hud| ours.is_some_and(|ours| ours.holds(hud.item)));
            }
            before.huds = before.huds.take().filter(|huds| !huds.is_empty());
            if !before.is_empty() {
                s.context = Some(context::Ask::Restore(before));
                s.undo_huds = true;
            }
        });
    }
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

/// Applies the workspace at `index` of the workspaces: its layout when it
/// names one that can be used, exactly as [`apply`] does; then, in the
/// frame the layout's windows are placed (the next one without a layout),
/// the windows it lists are opened and the other windows a workspace
/// switches are closed ([`frame`]). The arrangement it replaces, with the
/// windows open now, is kept for [`previous`].
pub fn apply_workspace(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<Report, StoreError> {
    let workspace: Workspace = live.store.workspace(index).ok_or(StoreError::Missing)?.clone();
    workspace.check().map_err(|_| StoreError::Invalid)?;
    let layout = workspace
        .layout
        .as_deref()
        .and_then(|name| live.store.find(name))
        .map(|(index, _)| index);
    let moves_window = layout
        .and_then(|index| live.store.layout(index))
        .is_some_and(|layout| layout.window.is_some());
    let mut before = arrangement(ctx, live, moves_window);
    // (its folder windows too, when the workspace changes them)
    let folders_now = workspace.folders.as_ref().and_then(|_| capture::folders(ctx));
    before.shown = Some((capture::open(ctx), live.store.active_workspace.clone(), folders_now));
    let applied = layout.and_then(|index| apply(ctx, live, index).ok());
    let mut report = Report {
        // (named, and not usable: a newer or damaged entry of the list)
        no_layout: workspace.layout.is_some() && applied.is_none(),
        ..applied.unwrap_or_default()
    };
    report.workspace = Some(workspace.name.clone());
    live.store.active_workspace = Some(workspace.name.clone());
    // its graphics, camera and HUDs: done by the app in the next frame,
    // with the HUDs the workspaces put on that it does not wear taken off
    // (only when it has HUDs of its own, and unless the user keeps them)
    let mut context = workspace.context();
    let undo_huds = !live.store.keep_huds;
    let frame = ctx.cumulative_frame_nr();
    session(ctx, |s| {
        if undo_huds && let (Some(ours), Some(huds)) = (&s.ours, &context.huds) {
            context.take_off = ours.take_off_for(huds);
        }
        s.context = Some(context).filter(|context| !context.is_empty()).map(context::Ask::Apply);
        s.undo_huds = undo_huds;
        s.queued_open = Some(QueuedOpen {
            open: workspace.open,
            folders: workspace.folders,
            frame,
        });
        s.previous = Some(before);
        s.report = Some(report.clone());
        s.notice = None;
    });
    Ok(report)
}

/// What *Save as a new workspace* keeps beside the open windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keep {
    /// The layout in use, applied with the workspace.
    pub layout: bool,
    /// *Remember Inventory folders*: the folder windows open now.
    pub folders: bool,
}

/// *Save as a new workspace*: the windows open now under `name`, with the
/// layout in use and the Inventory folder windows when `keep` says so; it
/// becomes the workspace in use.
pub fn save_new_workspace(ctx: &egui::Context, live: &mut Live, name: &str, keep: Keep) -> Result<usize, StoreError> {
    let layout = live.store.active().filter(|_| keep.layout).map(|layout| layout.name.clone());
    let folders = match keep.folders {
        true => Some(capture::folders(ctx).ok_or(StoreError::Invalid)?),
        false => None,
    };
    let index = live
        .store
        .add_workspace(Workspace::new(name, layout.as_deref(), capture::open(ctx)).with_folders(folders))?;
    session(ctx, |s| s.report = None);
    Ok(index)
}

/// *Update* of a workspace: the one at `index` takes the windows open now
/// (and the folder windows of now, when it remembers folders) and becomes
/// the workspace in use.
pub fn update_workspace(ctx: &egui::Context, live: &mut Live, index: usize) -> Result<(), StoreError> {
    let remembers = live.store.workspace(index).is_some_and(|workspace| workspace.folders.is_some());
    let folders = match remembers {
        true => Some(capture::folders(ctx).ok_or(StoreError::Invalid)?),
        false => None,
    };
    live.store.update_workspace(index, &capture::open(ctx))?;
    if folders.is_some() {
        live.store.set_workspace_folders(index, folders);
    }
    session(ctx, |s| s.report = None);
    Ok(())
}

/// *Update* of a workspace that also updates its layout: with `also_layout`,
/// the layout the workspace names first takes the arrangement of now
/// ([`update`]), then the workspace takes the windows open now
/// ([`update_workspace`]). When the layout cannot be updated, the error is
/// returned and the workspace is left as it was: one *Update*, one result.
pub fn update_workspace_with_layout(ctx: &egui::Context, live: &mut Live, index: usize, also_layout: bool) -> Result<(), StoreError> {
    if also_layout {
        let name = live.store.workspace(index).and_then(|workspace| workspace.layout.clone());
        let at = name.and_then(|name| live.store.find(&name).map(|(at, _)| at));
        update(ctx, live, at.ok_or(StoreError::Missing)?)?;
    }
    update_workspace(ctx, live, index)
}

/// The extra box of a workspace's *Update*, as a pure decision: None when it
/// is not offered (the workspace keeps the layout in use, or the layout it
/// names is gone or cannot be used); else whether it starts ticked, which is
/// when that layout is the one in use and modified, so its places differ
/// from what is on screen. Any other layout is not compared and starts
/// unticked.
pub fn layout_offer_default(names_layout: bool, usable: bool, in_use: bool, modified: bool) -> Option<bool> {
    (names_layout && usable).then_some(in_use && modified)
}

/// [`layout_offer_default`] for the workspace at `index`: the layout it
/// names, and whether the box starts ticked.
pub fn layout_offer(store: &Store, world: &ViewportSettings, index: usize) -> Option<(String, bool)> {
    let name = store.workspace(index)?.layout.as_deref();
    let found = name.and_then(|name| store.find(name));
    let ticked = layout_offer_default(
        name.is_some(),
        found.is_some(),
        found.is_some_and(|(at, _)| store.is_active(at)),
        modified(store, world),
    )?;
    found.map(|(_, layout)| (layout.name.clone(), ticked))
}

/// Whether the workspace in use no longer describes what is open: the
/// windows open now (`open`, as [`capture::open`] gives them) are not its
/// list. Worked out when asked; nothing is stored.
pub fn workspace_modified(store: &Store, open: &BTreeSet<String>) -> bool {
    store.active_workspace().is_some_and(|workspace| !workspace.matches(open))
}

/// *Remember Inventory folders* of the workspace at `index`, turned on
/// (the folder windows open now) or off. True when the store changed.
pub fn remember_folders(ctx: &egui::Context, live: &mut Live, index: usize, on: bool) -> bool {
    let folders = if on { capture::folders(ctx) } else { None };
    (folders.is_some() == on) && live.store.set_workspace_folders(index, folders)
}

/// How many Inventory folder windows are open now.
pub fn folder_windows_now(ctx: &egui::Context) -> usize {
    capture::folders(ctx).map_or(0, |folders| folders.windows.len())
}

/// The windows open now that a workspace switches.
pub fn open_now(ctx: &egui::Context) -> BTreeSet<String> {
    capture::open(ctx)
}

/// Whether [`previous`] has an arrangement to bring back.
pub fn has_previous(ctx: &egui::Context) -> bool {
    session(ctx, |s| s.previous.is_some())
}

/// What the last apply did.
pub fn report(ctx: &egui::Context) -> Option<Report> {
    session(ctx, |s| s.report.clone())
}

/// The app did what [`Outcome::context`] asked: what it did goes in the
/// report, the HUDs it asked to wear and to take off change the ones the
/// workspaces look after, and what it changed from is what *Previous
/// arrangement* puts back.
pub fn context_done(ctx: &egui::Context, mut done: context::Done) {
    let now = ctx.input(|i| i.time);
    session(ctx, |s| {
        if let Some(report) = &mut s.report {
            report.context = Some(done.told);
        }
        if let Some(swap) = &done.huds {
            context::Ours::changed(&mut s.ours, swap, now);
        }
        // (a switch that keeps the HUDs on leaves them to the user)
        if !s.undo_huds {
            done.before.huds = None;
        }
        if !done.before.is_empty()
            && let Some(previous) = &mut s.previous
        {
            previous.context = Some(done.before);
        }
    });
}

/// Logged out (the login screen): the HUDs the workspaces put on are not
/// looked after any more, and *Previous* puts none back.
pub fn logged_out(ctx: &egui::Context) {
    session(ctx, |s| {
        s.ours = None;
        if let Some(before) = s.previous.as_mut().and_then(|previous| previous.context.as_mut()) {
            before.huds = None;
        }
    });
}

/// The HUDs the workspaces put on and look after now (row 1.7).
#[cfg(test)]
pub fn ours(ctx: &egui::Context) -> Option<context::Ours> {
    session(ctx, |s| s.ours.clone())
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

/// Tells whether the card can be drawn this frame. Hidden (the interface
/// put away, mouselook: it could not be clicked), it waits: a card that
/// only tells counts its time from when it shows again.
pub fn set_notice_hidden(ctx: &egui::Context, hidden: bool) {
    session(ctx, |s| s.notice_hidden = hidden);
}

/// Why the switcher's last *Save new* or *Update* was refused, until
/// something it asks is done.
pub fn refused(ctx: &egui::Context) -> Option<StoreError> {
    session(ctx, |s| s.refused)
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

/// Applies the layout at `index` for a request: true when it was applied.
fn apply_asked(ctx: &egui::Context, live: &mut Live, index: Option<usize>) -> bool {
    index.is_some_and(|index| apply(ctx, live, index).is_ok())
}

/// Where the usable workspace `name` is in its list.
fn workspace_at(store: &Store, name: &str) -> Option<usize> {
    store.find_workspace(name).map(|(index, _)| index)
}

/// Runs what the menu, the card, the switcher, the toolbar's buttons and
/// the keyboard asked since the last frame.
fn run_requests(ctx: &egui::Context, live: &mut Live, outcome: &mut Outcome) {
    for request in menu::take(ctx) {
        // what the switcher was last refused is said until it asks again
        let mut refused = None;
        match request {
            Request::Apply(name) => {
                let index = live.store.find(&name).map(|(index, _)| index);
                outcome.save |= apply_asked(ctx, live, index);
            }
            Request::Previous => outcome.save |= previous(ctx, live),
            Request::Unlock => outcome.save |= unlock(live.store),
            Request::Preferences => outcome.preferences = true,
            // (an apply and a way back take the card away themselves)
            Request::Dismiss => {
                session(ctx, |s| s.notice = None);
                continue;
            }
            Request::Switcher => {
                switcher::toggle(ctx);
                continue;
            }
            Request::Toggle => match pick::toggle_target(live.store) {
                Ok(index) => outcome.save |= apply_asked(ctx, live, Some(index)),
                // its two layouts are chosen in Preferences
                Err(_) => outcome.preferences = true,
            },
            Request::Favorite(forward) => {
                let index = pick::next_favorite(live.store, forward);
                outcome.save |= apply_asked(ctx, live, index);
            }
            Request::Star(name, on) => outcome.save |= live.store.set_favorite(&name, on),
            Request::Update(name) => {
                let done = match live.store.find(&name).map(|(index, _)| index) {
                    Some(index) => update(ctx, live, index),
                    None => Err(StoreError::Missing),
                };
                outcome.save |= done.is_ok();
                refused = done.err();
            }
            Request::SaveNew(name, parts) => {
                let done = save_new(ctx, live, &name, parts);
                outcome.save |= done.is_ok();
                refused = done.err();
            }
            Request::ApplyWorkspace(name) => {
                let index = workspace_at(live.store, &name);
                outcome.save |= index.is_some_and(|index| apply_workspace(ctx, live, index).is_ok());
            }
            Request::StarWorkspace(name, on) => outcome.save |= live.store.set_favorite_workspace(&name, on),
            Request::UpdateWorkspace(name, also_layout) => {
                let done = match workspace_at(live.store, &name) {
                    Some(index) => update_workspace_with_layout(ctx, live, index, also_layout),
                    None => Err(StoreError::Missing),
                };
                outcome.save |= done.is_ok();
                refused = done.err();
            }
            Request::Strip(on) => outcome.save |= std::mem::replace(&mut live.store.strip, on) != on,
            Request::SaveWorkspace(name, keep) => {
                let done = save_new_workspace(ctx, live, &name, keep);
                outcome.save |= done.is_ok();
                refused = done.err();
            }
        }
        session(ctx, |s| s.refused = refused);
    }
}

/// Each frame, before the interface's pass: `seen` is the viewer window
/// now (None: there is none to drive). Runs what the menu and the card
/// asked, carries on the viewer window's steps and keeps its lock, places
/// the windows of an apply once the viewer window is in place, asks again
/// at the start for the windows still waiting from the last session, looks
/// at the monitors, and twice a second looks at the windows for the
/// "modified" mark.
pub fn frame(ctx: &egui::Context, live: &mut Live, seen: Option<Seen>, shell: &mut dyn Shell) -> Outcome {
    let mut outcome = Outcome::default();
    // (first: a layout saved in this frame reads which windows are open)
    capture::set_open(ctx, windows::all().iter().filter(|w| shell.is_open(w)).map(|w| w.id).collect());
    capture::set_folders(
        ctx,
        capture::FolderView {
            account: shell.account(),
            windows: shell.folder_windows(),
        },
    );
    context::set_view(ctx, hud_view(live.store, shell));
    run_requests(ctx, live, &mut outcome);
    let now = ctx.cumulative_frame_nr();
    let mut s = session(ctx, std::mem::take);
    look_after_huds(&mut s, shell, now, ctx.input(|i| i.time));
    let found = s.viewer.frame(ctx, live.store, seen, &mut outcome.commands);
    outcome.save |= found.save;
    let mut apply_now = None;
    let t = ctx.input(|i| i.time);
    if let Some(setup) = &found.setup {
        (s.notice, apply_now) = on_setup(live.store, setup, found.unlocked);
        s.notice_at = t;
    }
    // a card that is not drawn has not been read yet
    if s.notice_hidden {
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
        outcome.save |= windows_frame(ctx, live, &mut s, &room, shell);
    }
    let modified = if live.store.strip {
        menu::Modified {
            layout: modified(live.store, live.world),
            workspace: workspace_modified(live.store, &capture::open(ctx)),
        }
    } else {
        menu::Modified::default()
    };
    let stamp = menu::stamp(live.store, s.previous.is_some(), modified);
    if stamp != std::mem::replace(&mut s.menu, stamp) {
        menu::publish(ctx, live.store, s.previous.is_some(), modified);
    }
    session(ctx, |slot| *slot = s);
    // the switcher draws a copy of the store, kept up while it is open
    switcher::publish(ctx, live.store, live.world, &capture::open(ctx));
    // a workspace's context, asked by the requests of this frame or by
    // Preferences in the last
    outcome.context = session(ctx, |s| s.context.take());
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

/// The HUDs the workspaces put on, looked after (row 1.7): forgotten with
/// another account; every [`HUDS_EVERY`] frames, the ones that arrived
/// are marked and the ones taken off since are forgotten.
fn look_after_huds(s: &mut Session, shell: &dyn Shell, frame: u64, now: f64) {
    let account = shell.account();
    if s.ours.as_ref().is_some_and(|ours| Some(ours.account) != account) {
        s.ours = None;
    }
    if frame.is_multiple_of(HUDS_EVERY)
        && let Some(ours) = &mut s.ours
        && let Some(on) = shell.huds_on()
        && !ours.observe(&on, now)
    {
        s.ours = None;
    }
}

/// What Preferences shows of HUDs: the HUDs worn now and the names of the
/// items the workspaces hold for the account logged in.
fn hud_view(store: &Store, shell: &dyn Shell) -> context::HudView {
    let account = shell.account();
    let name = |item: Uuid| shell.item_name(item).unwrap_or_default();
    let worn = shell.worn_huds().into_iter().map(|hud| (hud, name(hud.item))).collect();
    let names = store
        .workspaces
        .iter()
        .filter_map(|record| record.workspace()?.huds.as_ref())
        .filter(|huds| Some(huds.account) == account)
        .flat_map(|huds| &huds.items)
        .filter_map(|hud| Some((hud.item, shell.item_name(hud.item)?)))
        .collect();
    context::HudView { account, worn, names }
}

#[derive(Default)]
struct FolderCounts {
    opened: usize,
    closed: usize,
    missing: usize,
    blocked: usize,
    other_account: bool,
}

/// Puts the Inventory folder windows as `folders` has them: skipped whole
/// for another account; a folder the Inventory does not have is skipped
/// and counted; a window already open on a folder is reused, another one
/// opened (unless RLVa hides the Inventory); each takes its saved place,
/// fitted to `room`; the folder windows left over are closed.
fn folder_windows(ctx: &egui::Context, room: &Room, folders: &Folders, shell: &mut dyn Shell, counts: &mut FolderCounts) {
    if shell.account() != Some(folders.account) {
        counts.other_account = true;
        return;
    }
    let mut open = shell.folder_windows();
    for saved in &folders.windows {
        if !shell.has_folder(saved.folder) {
            counts.missing += 1;
            continue;
        }
        let window = match open.iter().position(|(_, folder)| *folder == saved.folder) {
            Some(at) => open.swap_remove(at).0,
            None => match shell.open_folder(ctx, saved.folder) {
                Some(window) => {
                    counts.opened += 1;
                    window
                }
                None => {
                    counts.blocked += 1;
                    continue;
                }
            },
        };
        let fitted = fit(&saved.place, &folders.frame, &room.frame);
        crate::ui::widgets::place_floater(ctx, &window, room.origin + fitted.pos.to_vec2(), fitted.size);
        if fitted.minimized {
            crate::ui::widgets::minimize_floater(ctx, &window);
        }
    }
    for (window, _) in open {
        shell.close_folder(&window);
        counts.closed += 1;
    }
}

/// The windows' part of a frame, in the interface `room`. True when the
/// store changed.
fn windows_frame(ctx: &egui::Context, live: &mut Live, s: &mut Session, room: &Room, shell: &mut dyn Shell) -> bool {
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
    let layout = s.queued.take_if(|queued| queued.frame < now && !viewer_busy);
    // a workspace's windows: with its layout's, or alone
    let shown = match (&s.queued, viewer_busy) {
        (None, false) => s.queued_open.take_if(|queued| queued.frame < now),
        _ => None,
    };
    if layout.is_some() || shown.is_some() {
        let (mut fitted, mut opened, mut blocked, mut closed) = (0, 0, 0, 0);
        // the workspace's list first: what it has open is opened the app's
        // way, the other windows it switches are closed
        if let Some(shown) = &shown {
            for window in workspace::switchable() {
                match (shown.open.contains(window.id), shell.is_open(window)) {
                    (true, false) => match shell.open(ctx, window) {
                        true => opened += 1,
                        false => blocked += 1,
                    },
                    (false, true) => {
                        shell.close(ctx, window);
                        closed += 1;
                    }
                    _ => {}
                }
            }
            // its Inventory folder windows: the ones it remembers are
            // opened (one already open on the folder is reused) and
            // placed, the other folder windows are closed
            if let Some(folders) = &shown.folders {
                let mut counts = FolderCounts::default();
                folder_windows(ctx, room, folders, shell, &mut counts);
                opened += counts.opened;
                closed += counts.closed;
                if let Some(report) = &mut s.report {
                    report.folders_missing = counts.missing;
                    report.folders_blocked = counts.blocked;
                    report.folders_other_account = counts.other_account;
                }
            }
        }
        if let Some(queued) = &layout {
            s.targets.clear();
            for (id, place) in &queued.windows.windows {
                if let Some(window) = windows::find(id) {
                    // open when the layout was saved, closed now: opened the
                    // app's way, before its place is asked (opening a window
                    // expands it; the request minimizes it again if saved
                    // so). With a workspace, its list has decided already.
                    if shown.is_none() && place.open && !shell.is_open(window) {
                        match shell.open(ctx, window) {
                            true => opened += 1,
                            false => blocked += 1,
                        }
                    }
                    let place = fit(place, &queued.windows.frame, &room.frame);
                    fitted += usize::from(place.changed);
                    request(ctx, s, window, room, &place);
                }
            }
        }
        if let Some(report) = &mut s.report {
            if layout.is_some() {
                report.fitted = fitted;
            }
            report.opened = opened;
            report.blocked = blocked;
            report.closed = closed;
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
