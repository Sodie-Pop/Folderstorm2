//! The window's side of checkpoint 9: each monitor in a window of its
//! own, and the cycling monitor (session-host plan, checkpoint 9). The
//! rules are the `folderstorm-session` crate's (`monitor_windows`, the
//! desk's monitors); this module opens, places, draws and closes the
//! windows.
//!
//! - **A monitor's window** is compact: a thin header (the character's
//!   colour and name, then Switch, Chat, Pin and Options as icon
//!   buttons), the picture, and, when asked for, a chat part: that
//!   character's conversations as chips (`chips.rs`), the lines of the
//!   one chosen and a box that speaks **as that character** (the shared
//!   chat's own: `chat_ui::compose`).
//! - **Switch swaps**: the desk moves the monitor to the character that
//!   left the front ([`Window_::follow_moves`]); the window, its place,
//!   its pin and its chat part stay.
//! - **The cycling monitor** shows each background character in turn; its
//!   turn does not pass while the user types in its chat.
//! - **The options are a sheet inside the window**: a window this small
//!   cannot show a menu taller than itself.
//! - **Closing a monitor's window closes that monitor**, and nothing
//!   else: its viewer goes back to 3D off.
//! - **As every window of the host**: drawn on the processor, opened
//!   where a screen shows it, and nothing here takes the keyboard by
//!   itself (the windows found open when the host starts open without
//!   it). The picture senses neither clicks nor keys, and no picture is
//!   written anywhere.

use super::super::{chat_ui, chips, contacts_ui, faces};
use super::panels::{View, Which};
use super::{Act as HostAct, BUTTON, Window_};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::chat::Viewing;
use ::folderstorm_session::host::{Slot, Watch};
use ::folderstorm_session::ipc::{Conversation, MAX_SLOTS};
use ::folderstorm_session::monitor::{Rate, Size, Spec};
use ::folderstorm_session::monitor_windows::{self as rules, CHAT, Cycle, HEADER, INTERVALS_S, MIN_PICTURE, MIN_WIDTH};
use ::folderstorm_session::panels::{self, Place};
use aurora_i18n::tr;
use egui::{Rect, RichText, Vec2};
use std::time::Instant;
use winit::{
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowLevel},
};

/// A monitor's window: a character's own, or the cycling monitor's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in super::super) enum Seat {
    Slot(usize),
    Cycling,
}

impl Seat {
    fn all() -> impl Iterator<Item = Seat> {
        (0..MAX_SLOTS).map(Seat::Slot).chain([Seat::Cycling])
    }

    pub fn which(self) -> Which {
        match self {
            Seat::Slot(slot) => Which::Monitor(slot),
            Seat::Cycling => Which::Cycling,
        }
    }

    pub fn of(which: Which) -> Option<Seat> {
        match which {
            Which::Monitor(slot) => Some(Seat::Slot(slot)),
            Which::Cycling => Some(Seat::Cycling),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Seat::Slot(slot) => slot.min(MAX_SLOTS - 1),
            Seat::Cycling => MAX_SLOTS,
        }
    }

    /// The window's number for `chat::Chat::look_from` (0 is the shared
    /// chat's).
    pub(super) fn look_key(self) -> u8 {
        self.index() as u8 + 1
    }

    /// The window's name in a capture's file name and in the log.
    pub fn key(self) -> String {
        match self {
            Seat::Slot(slot) => format!("monitor-{}", slot + 1),
            Seat::Cycling => "cycling".to_owned(),
        }
    }
}

/// What one monitor's window keeps beside what is saved of it.
#[derive(Default)]
struct Pane {
    view: Option<View>,
    /// The conversation its chat part shows, for each character (the
    /// cycling monitor shows them all in turn).
    shown: [Conversation; MAX_SLOTS],
    /// The "+N" list is open.
    more: bool,
    /// The options' sheet is open.
    options: bool,
    /// The pin, the title and the size the window was last given.
    level: bool,
    title: String,
    fitted: Option<(Size, bool)>,
    /// What its chat part showed in its last drawing.
    viewing: Viewing,
    /// The chat's field had the keyboard in its last drawing.
    typing: bool,
    /// Checkpoint 11: the "New IM" or "Friends" sheet in the lines'
    /// place, for that character.
    sheet: Option<(usize, contacts_ui::Sheet)>,
    /// Checkpoint 11: a web link that waits for the user's yes.
    link: Option<chat_ui::LinkAsk>,
    /// Tests: where that question was drawn last.
    #[cfg(test)]
    asked_at: Option<Rect>,
}

/// What the host's windows keep for the monitors' own.
#[derive(Default)]
pub(super) struct Monitors {
    /// One per character, then the cycling monitor's.
    panes: [Pane; MAX_SLOTS + 1],
    /// The cycling monitor's turn.
    pub cycle: Cycle,
    /// A text typed in the cycling monitor's box that is not sent yet:
    /// the character and the conversation it was typed for.
    typed: Option<(usize, Conversation)>,
    /// The windows found open when the host started have their windows.
    restored: bool,
}

impl Monitors {
    /// Checkpoint 11: a conversation opened from the window whose chat
    /// number is `window`: its chat part shows it.
    pub(super) fn opened(&mut self, window: u8, slot: usize, conversation: Conversation) {
        if let Some(seat) = Seat::all().find(|seat| seat.look_key() == window)
            && slot < MAX_SLOTS
        {
            let pane = self.pane_mut(seat);
            pane.shown[slot] = conversation;
            pane.sheet = None;
            pane.more = false;
        }
    }

    /// Tests: the seat's sheet for that character (or none), and what its
    /// chat part shows of it.
    #[cfg(test)]
    pub(super) fn test_sheet(&mut self, seat: Seat, slot: usize, which: Option<::folderstorm_session::contacts::Which>) {
        self.pane_mut(seat).sheet = which.map(|which| {
            (
                slot,
                contacts_ui::Sheet {
                    which,
                    query: String::new(),
                },
            )
        });
    }

    /// Tests: a web link that waits for the user's yes in the seat's chat
    /// part, and where its question was drawn last.
    #[cfg(test)]
    pub(super) fn test_link(&mut self, seat: Seat, link: Option<chat_ui::LinkAsk>) {
        self.pane_mut(seat).link = link;
    }

    #[cfg(test)]
    pub(super) fn test_asked_at(&self, seat: Seat) -> Option<Rect> {
        self.pane(seat).asked_at
    }

    #[cfg(test)]
    pub(super) fn test_shown(&self, seat: Seat, slot: usize) -> (Conversation, bool) {
        let pane = self.pane(seat);
        (pane.shown[slot], pane.sheet.is_some())
    }

    fn pane(&self, seat: Seat) -> &Pane {
        &self.panes[seat.index()]
    }

    fn pane_mut(&mut self, seat: Seat) -> &mut Pane {
        &mut self.panes[seat.index()]
    }

    pub fn view(&self, seat: Seat) -> Option<&View> {
        self.pane(seat).view.as_ref()
    }

    pub fn view_mut(&mut self, seat: Seat) -> Option<&mut View> {
        self.pane_mut(seat).view.as_mut()
    }

    /// Every monitor's window that is open, with its seat.
    pub fn views(&self) -> impl Iterator<Item = (Seat, &View)> {
        Seat::all().filter_map(|seat| self.view(seat).map(|view| (seat, view)))
    }

    pub fn views_mut(&mut self) -> impl Iterator<Item = &mut View> {
        self.panes.iter_mut().filter_map(|pane| pane.view.as_mut())
    }

    /// Gives a seat a view without a window, as `sync_monitors` gives it
    /// one with: for the tests' drawings.
    #[cfg(test)]
    pub fn test_open(&mut self, seat: Seat, view: View) {
        self.pane_mut(seat).view = Some(view);
    }
}

/// What a click in a monitor's window, or on the monitors' page, asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// Bring that character to the front (the window then shows the one
    /// that left it).
    Switch(Seat, usize),
    /// The size and rate of that window's pictures.
    Spec(Seat, Spec),
    /// Its chat part, shown or not; its pin; its options' sheet.
    Chat(Seat, bool),
    Pin(Seat, bool),
    Options(Seat, bool),
    /// The size and rate a new monitor opens with.
    Defaults(Spec),
    /// The cycling monitor: its turn in seconds, "Pause cycling while
    /// typing", next (true) or previous, paused or not, open or closed.
    Interval(u32),
    PauseTyping(bool),
    Step(bool),
    Pause(bool),
    Cycling(bool),
}

/// The window's level: over the others when pinned.
fn level(pinned: bool) -> WindowLevel {
    if pinned { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal }
}

/// Where a window is, when that is a place to open it again.
fn place(view: &View) -> Option<Place> {
    let window = view.window.as_ref()?;
    if window.is_minimized().unwrap_or(false) || window.is_maximized() {
        return None;
    }
    let size = window.inner_size();
    Some(Place {
        // (not told on Wayland: the system places the window)
        position: window.outer_position().ok().map(|at| [at.x, at.y]),
        size: [size.width, size.height],
    })
}

/// A size as its choice shows it.
fn size_text(size: Size) -> String {
    format!("{} × {}", size.dims().0, size.dims().1)
}

/// A turn of the cycling monitor as its choice shows it.
fn interval_text(seconds: u32) -> String {
    tr!("folderstorm-cycling-seconds", seconds = seconds)
}

/// One of the header's icon buttons: lit while `on`.
fn tool(ui: &mut egui::Ui, p: &Palette, icon: &str, tip: &str, on: bool, enabled: bool) -> egui::Response {
    let kind = if on { ActionKind::Primary } else { ActionKind::Flat };
    widgets::action_button_if(ui, p, icon, "", kind, TOOL, enabled).on_hover_text(tip)
}

/// The size of a header's button, and the room between two.
const TOOL: Vec2 = Vec2::new(26.0, 22.0);
const TOOL_GAP: f32 = 3.0;
/// The least height of a chat part that is worth drawing.
const MIN_CHAT: f32 = 96.0;
/// The room the chat's box keeps under the lines.
const BOX_ROOM: f32 = 50.0;

/// Where the picture and the chat part go in a monitor's body, under its
/// header: the picture's room, and the chat part's when it has one.
///
/// - **Chat off**, or no room for a chat part worth drawing: the picture's
///   room is the whole body, the picture centred in it with bands where
///   its proportions leave them.
/// - **Chat on**: the chat part has at least `wanted` (or what the least
///   picture leaves); the picture sits at the top, as tall as its
///   proportions make it at the body's width (never less than the least
///   picture), and **the chat part takes all the rest**: no band above or
///   under the picture. Without a picture (`picture` is None: a reason is
///   shown in its place, such as "This is the active character"), that
///   reason keeps the least picture's room and the chat the rest.
/// - A window wider than the picture's proportions keeps its bands at the
///   sides and the chat under the picture: a chat beside it would be a
///   narrow column of lines, and its window's size follows the chat part
///   under the picture (`rules::natural_size`, `toggled_height`).
///
/// `picture`: the picture's size in pixels.
fn split(body: Rect, chat: bool, wanted: f32, picture: Option<[usize; 2]>) -> (Rect, Option<Rect>) {
    let least = MIN_PICTURE as f32;
    let chat_height = if chat { wanted.min(body.height() - least) } else { 0.0 };
    if chat_height < MIN_CHAT {
        return (body, None);
    }
    let natural = match picture {
        Some([w, h]) => body.width() * h.max(1) as f32 / w.max(1) as f32,
        None => least,
    };
    // (the picture's room is never more than the chat part's least leaves)
    let room = natural.max(least).min(body.height() - chat_height).round();
    let picture = Rect::from_min_max(body.min, egui::pos2(body.right(), body.top() + room));
    let chat = Rect::from_min_max(egui::pos2(body.left(), picture.bottom()), body.max);
    (picture, Some(chat))
}

/// A child of `ui` in `rect`, clipped to it, laid out from the top.
fn child(ui: &mut egui::Ui, rect: Rect, inside: Vec2) -> egui::Ui {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(inside))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

impl Window_ {
    /// A seat's monitor is open.
    fn seat_open(&self, seat: Seat) -> bool {
        match seat {
            Seat::Slot(slot) => self.host.desk.slot(slot).is_some_and(|s| s.monitor.is_some()),
            Seat::Cycling => self.host.desk.monitors().cycling.open,
        }
    }

    /// The size and rate of a seat's pictures.
    fn seat_spec(&self, seat: Seat) -> Spec {
        match seat {
            Seat::Slot(slot) => self.host.desk.slot(slot).and_then(|s| s.monitor).unwrap_or_default(),
            Seat::Cycling => self.host.desk.monitors().cycling.spec,
        }
    }

    /// What is remembered of a seat's window.
    fn seat_window(&self, seat: Seat) -> rules::Window {
        match seat {
            Seat::Slot(slot) => self.host.desk.slot(slot).map(|s| s.monitor_window).unwrap_or_default(),
            Seat::Cycling => self.host.desk.monitors().cycling.window,
        }
    }

    fn edit_seat_window(&mut self, seat: Seat, edit: impl FnOnce(&mut rules::Window)) {
        match seat {
            Seat::Slot(slot) => {
                self.host.desk.edit_monitor_window(slot, edit);
            }
            Seat::Cycling => {
                self.host.desk.edit_monitors(|monitors| edit(&mut monitors.cycling.window));
            }
        }
    }

    /// Closes a seat's monitor (its window goes at the next wait).
    fn close_seat(&mut self, seat: Seat) {
        match seat {
            Seat::Slot(slot) => self.host.desk.set_monitor(slot, None),
            Seat::Cycling => self.host.desk.set_cycling(false),
        }
    }

    /// The character a seat shows now.
    fn seat_slot(&self, seat: Seat) -> Option<usize> {
        match seat {
            Seat::Slot(slot) => Some(slot),
            Seat::Cycling => self.monitors.cycle.shown(),
        }
    }

    /// The title of a seat's window: the character's name, or "Cycling
    /// monitor".
    fn monitor_title(&self, seat: Seat) -> String {
        match seat {
            Seat::Slot(slot) => format!("{} — {}", self.name(slot), tr!("folderstorm-monitor")),
            Seat::Cycling => tr!("folderstorm-bar-monitors-cycling").to_owned(),
        }
    }

    /// The monitors the desk moved from one character to another keep
    /// their windows: each window, with what it keeps, goes with its
    /// monitor. (Before the windows are waited on, never while one of
    /// them is drawn: a view that is drawn has lent its pixels out.)
    pub(super) fn follow_moves(&mut self) {
        for (from, to) in self.host.desk.take_monitor_moves() {
            if from >= MAX_SLOTS || to >= MAX_SLOTS || from == to {
                continue;
            }
            log::info!("session host: the monitor of character {} now shows character {}", from + 1, to + 1);
            // (a window the one that left the front still had is gone:
            // its monitor was closed)
            if let Some(mut stale) = self.monitors.panes[to].view.take() {
                stale.detach();
            }
            self.monitors.panes.swap(from, to);
            let (key_from, key_to) = (Seat::Slot(from).look_key(), Seat::Slot(to).look_key());
            self.host.chat.look_from(key_from, Viewing::Nothing, false);
            self.host.chat.look_from(key_to, Viewing::Nothing, false);
            let pane = &mut self.monitors.panes[to];
            pane.viewing = Viewing::Nothing;
            pane.more = false;
            pane.options = false;
            pane.typing = false;
            if let Some(view) = &mut pane.view {
                view.redraw = true;
                // (it shows that character from now on: on screen, as it
                // was, until its next drawing says otherwise)
                view.monitors.seen = std::array::from_fn(|slot| slot == to);
            }
        }
    }

    /// The monitors' windows follow the desk: one opens for a monitor
    /// that is open and has none, the window of a monitor that closed
    /// goes, and each takes its pin, its title and, after a choice of
    /// size or of the chat part, its size. The windows opened by the
    /// first call are those found open when the host starts: they do not
    /// take the keyboard.
    pub(super) fn sync_monitors(&mut self, event_loop: &ActiveEventLoop) {
        let restoring = !std::mem::replace(&mut self.monitors.restored, true);
        self.follow_moves();
        for seat in Seat::all() {
            match (self.seat_open(seat), self.monitors.view(seat).is_some()) {
                (true, false) => match self.open_monitor_window(event_loop, seat, !restoring) {
                    Ok(view) => {
                        log::info!("session host: {} in its window", seat.key());
                        let (spec, window) = (self.seat_spec(seat), self.seat_window(seat));
                        let title = self.monitor_title(seat);
                        let pane = self.monitors.pane_mut(seat);
                        pane.view = Some(view);
                        pane.level = window.pinned;
                        pane.title = title;
                        pane.fitted = Some((spec.size, window.chat));
                        pane.options = false;
                        self.redraw_all();
                    }
                    Err(e) => {
                        // no window: no monitor (its viewer is asked nothing)
                        log::warn!("session host: no window for {} ({e}): closed", seat.key());
                        self.close_seat(seat);
                    }
                },
                (false, true) => {
                    log::info!("session host: {} closed", seat.key());
                    self.drop_monitor_window(seat);
                    self.redraw_all();
                }
                _ => {}
            }
            let (spec, config) = (self.seat_spec(seat), self.seat_window(seat));
            let title = self.monitor_title(seat);
            let pane = self.monitors.pane_mut(seat);
            let Some(window) = pane.view.as_ref().and_then(|view| view.window.as_ref()) else {
                continue;
            };
            if std::mem::replace(&mut pane.level, config.pinned) != config.pinned {
                window.set_window_level(level(config.pinned));
            }
            if pane.title != title {
                window.set_title(&title);
                pane.title = title;
            }
            let fitted = Some((spec.size, config.chat));
            if pane.fitted != fitted {
                // a size was chosen: the window takes the size that shows
                // it; the chat part alone: the window grows or shrinks by
                // it, as wide as the user made it
                let was = std::mem::replace(&mut pane.fitted, fitted);
                let scale = window.scale_factor();
                let wanted = match was {
                    Some((size, _)) if size == spec.size => {
                        let now = window.inner_size();
                        [now.width, rules::toggled_height(now.height, config.chat, scale)]
                    }
                    _ => rules::natural_size(spec, config.chat, scale),
                };
                let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(wanted[0], wanted[1]));
            }
        }
    }

    /// A seat's window goes, with what it kept; what is saved of it stays.
    fn drop_monitor_window(&mut self, seat: Seat) {
        let pane = self.monitors.pane_mut(seat);
        if let Some(mut view) = pane.view.take() {
            view.detach();
        }
        *pane = Pane {
            shown: pane.shown,
            ..Pane::default()
        };
        self.host.chat.look_from(seat.look_key(), Viewing::Nothing, false);
        if seat == Seat::Cycling {
            self.monitors.typed = None;
        }
    }

    fn open_monitor_window(&self, event_loop: &ActiveEventLoop, seat: Seat, active: bool) -> anyhow::Result<View> {
        use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
        let (spec, config) = (self.seat_spec(seat), self.seat_window(seat));
        let scale = event_loop.primary_monitor().map_or(1.0, |monitor| monitor.scale_factor());
        let [width, height] = rules::natural_size(spec, config.chat, scale);
        let mut attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(self.monitor_title(seat))
            .with_inner_size(PhysicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(MIN_WIDTH, HEADER + MIN_PICTURE))
            .with_window_level(level(config.pinned))
            .with_active(active);
        if let Some(saved) = config.place {
            let place = panels::on_screen(saved, &super::panels::screens(event_loop));
            if place != saved {
                log::info!("session host: {}'s window was off the screens: brought back", seat.key());
            }
            attributes = attributes.with_inner_size(PhysicalSize::new(place.size[0], place.size[1]));
            if let Some([x, y]) = place.position {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
        }
        let mut view = self.look.view();
        view.attach(event_loop, event_loop.create_window(attributes)?)?;
        Ok(view)
    }

    /// What a monitor's window is told, before the interface hears it.
    /// True when the window is gone (closed: so is its monitor).
    pub(super) fn monitor_event(&mut self, seat: Seat, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => {
                // that monitor closes; never the host's or a character's end
                self.close_seat(seat);
                self.drop_monitor_window(seat);
                log::info!("session host: {}'s window closed", seat.key());
                self.redraw_all();
                true
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(place) = self.monitors.view(seat).and_then(place) {
                    self.edit_seat_window(seat, |window| {
                        window.placed(place);
                    });
                }
                false
            }
            WindowEvent::Focused(false) => {
                // nothing is read in a window the user has left
                let viewing = self.monitors.pane(seat).viewing;
                self.host.chat.look_from(seat.look_key(), viewing, false);
                false
            }
            _ => false,
        }
    }

    /// A monitor's window could not be drawn: its monitor closes, the
    /// host stays.
    pub(super) fn monitor_failed(&mut self, seat: Seat, error: &anyhow::Error) {
        log::warn!("session host: {}'s window is not drawn ({error}): closed", seat.key());
        self.close_seat(seat);
    }

    /// Now and then: the cycling monitor's turn. The turn does not pass
    /// while the user types in its chat, when they chose so.
    pub(super) fn cycling_tick(&mut self, now: Instant) {
        let cycling = self.host.desk.monitors().cycling;
        if !cycling.open {
            self.monitors.typed = None;
            return;
        }
        let candidates = self.host.desk.cycling_candidates();
        let hold = cycling.pause_typing && self.cycling_typing();
        let before = self.monitors.cycle.shown();
        if self.monitors.cycle.tick(&candidates, cycling.interval(), hold, now) != before
            && let Some(view) = self.monitors.view_mut(Seat::Cycling)
        {
            view.redraw = true;
        }
    }

    /// The user types in the cycling monitor's chat: its field has the
    /// keyboard, or holds a text typed there for the character shown that
    /// is not sent yet.
    fn cycling_typing(&mut self) -> bool {
        // (sent, erased, or the conversation is gone: no text any more)
        if self
            .monitors
            .typed
            .is_some_and(|(slot, conversation)| self.host.chat.draft(slot, conversation).is_empty())
        {
            self.monitors.typed = None;
        }
        let pane = self.monitors.pane(Seat::Cycling);
        let at_the_field = pane.typing && pane.view.as_ref().is_some_and(|view| view.focused);
        let shown = self.monitors.cycle.shown();
        at_the_field || self.monitors.typed.is_some_and(|(slot, _)| Some(slot) == shown)
    }

    /// The character the cycling monitor shows on screen now, for the
    /// desk: none while its window is minimised, covered or shows no
    /// picture.
    pub(super) fn cycling_sight(&self) -> Option<usize> {
        let sight = self.monitors.view(Seat::Cycling)?.sight();
        self.monitors.cycle.shown().filter(|slot| sight.visible()[*slot])
    }

    pub(super) fn monitors_apply(&mut self, act: Act, now: Instant) {
        match act {
            Act::Switch(seat, slot) => {
                let before = self.host.desk.active();
                self.host.desk.switch(slot, now);
                // the cycling monitor shows the one that left the front,
                // as a monitor's own window does
                if seat == Seat::Cycling
                    && self.host.desk.active() == Some(slot)
                    && let Some(before) = before.filter(|before| *before != slot)
                {
                    self.monitors.cycle.show(before, now);
                }
            }
            Act::Spec(Seat::Slot(slot), spec) => self.host.desk.set_monitor(slot, Some(spec)),
            Act::Spec(Seat::Cycling, spec) => {
                self.host.desk.edit_monitors(|monitors| monitors.cycling.spec = spec);
            }
            Act::Chat(seat, chat) => self.edit_seat_window(seat, |window| window.chat = chat),
            Act::Pin(seat, pinned) => self.edit_seat_window(seat, |window| window.pinned = pinned),
            Act::Options(seat, open) => self.monitors.pane_mut(seat).options = open,
            Act::Defaults(spec) => {
                self.host.desk.edit_monitors(|monitors| monitors.defaults = spec);
            }
            Act::Interval(seconds) => {
                self.host.desk.edit_monitors(|monitors| monitors.cycling.interval_s = seconds);
            }
            Act::PauseTyping(pause) => {
                self.host.desk.edit_monitors(|monitors| monitors.cycling.pause_typing = pause);
            }
            Act::Step(forward) => {
                let candidates = self.host.desk.cycling_candidates();
                self.monitors.cycle.step(&candidates, forward, now);
                // the user moved on: a text left unsent no longer holds
                // the turn (it stays its character's draft)
                self.monitors.typed = None;
            }
            Act::Pause(paused) => self.monitors.cycle.set_paused(paused, now),
            Act::Cycling(open) => self.host.desk.set_cycling(open),
        }
    }

    /// A monitor's window: its header, then its options, or its picture
    /// and its chat part.
    pub(super) fn monitor_window(&mut self, ui: &mut egui::Ui, seat: Seat, now: Instant, acts: &mut Vec<HostAct>) {
        let p = self.palette;
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, p.panel);
        let slot = self.seat_slot(seat);
        let state: Option<(usize, Slot)> = slot.and_then(|slot| self.host.desk.slot(slot).map(|state| (slot, state.clone())));
        let config = self.seat_window(seat);
        let header = Rect::from_min_size(full.min, Vec2::new(full.width(), HEADER as f32));
        let body = Rect::from_min_max(egui::pos2(full.left(), header.bottom()), full.max);
        self.monitor_header(ui, &p, header, seat, state.as_ref(), config, now, acts);
        let mut viewing = Viewing::Nothing;
        if self.monitors.pane(seat).options {
            self.monitor_options(ui, &p, body, seat, config, acts);
        } else {
            // the chat part under the picture, when there is room for
            // both, with all the room the picture does not use; while the
            // "+N" list is open it takes what the least picture leaves
            // (checkpoint 11: and so does a "New IM" or "Friends" sheet;
            // a link's question makes its own room: `monitor_chat`)
            let pane = self.monitors.pane(seat);
            let wanted = if pane.more || pane.sheet.is_some() {
                body.height()
            } else {
                CHAT as f32
            };
            // (no character shown: no chat part, the reason has the body)
            let picture = state
                .as_ref()
                .and_then(|(slot, _)| self.host.screens[*slot].shown)
                .map(|shown| shown.size);
            let (picture, chat) = split(body, config.chat && state.is_some(), wanted, picture);
            self.monitor_picture_in(ui, &p, picture, seat, state.as_ref(), now);
            if let Some(room) = chat {
                viewing = self.monitor_chat(ui, &p, room, seat, state.as_ref(), now, acts);
            } else {
                self.monitors.pane_mut(seat).typing = false;
            }
        }
        // what its chat part shows is read while this window has the
        // keyboard
        let focused = self.view(seat.which()).is_some_and(|view| view.focused);
        self.monitors.pane_mut(seat).viewing = viewing;
        self.host.chat.look_from(seat.look_key(), viewing, focused);
    }

    /// The header: who is shown, then the buttons.
    #[allow(clippy::too_many_arguments)]
    fn monitor_header(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        rect: Rect,
        seat: Seat,
        state: Option<&(usize, Slot)>,
        config: rules::Window,
        now: Instant,
        acts: &mut Vec<HostAct>,
    ) {
        let act = |act: Act| HostAct::Monitors(act);
        let active = self.host.desk.active();
        let options = self.monitors.pane(seat).options;
        let cycling = seat == Seat::Cycling;
        let buttons = if cycling { 7.0 } else { 4.0 };
        let buttons_width = buttons * (TOOL.x + TOOL_GAP);
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(6.0, (rect.height() - TOOL.y) / 2.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.set_clip_rect(rect.intersect(ui.clip_rect()));
        row.spacing_mut().item_spacing = Vec2::new(TOOL_GAP, 0.0);
        // who: the colour, the alias or name; the real name and how the
        // monitor does in the hover
        let name_room = (row.available_width() - buttons_width).max(24.0);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        row.allocate_ui_with_layout(Vec2::new(name_room, TOOL.y), layout, |ui| {
            ui.set_min_width(name_room);
            match state {
                Some((slot, state)) => {
                    let face = faces::face(p, state);
                    faces::dot(ui, p, face.tint);
                    let (status, notice) = self.monitor_status(seat, *slot, state, now);
                    if notice {
                        faces::sign(ui, "alert-triangle-filled", p.amber, &status);
                    }
                    let mut tip = face.real.clone().unwrap_or_default();
                    if !status.is_empty() {
                        if !tip.is_empty() {
                            tip.push('\n');
                        }
                        tip.push_str(&status);
                    }
                    let colour = if active == Some(*slot) { p.violet_light } else { p.ink };
                    let name = ui.add(egui::Label::new(RichText::new(&face.name).strong().color(colour)).truncate());
                    if !tip.is_empty() {
                        name.on_hover_text(tip);
                    }
                }
                None => {
                    ui.add(egui::Label::new(RichText::new(tr!("folderstorm-cycling-nobody-short")).color(p.muted)).truncate());
                }
            }
        });
        if cycling {
            let some = state.is_some();
            let cycle = self.monitors.cycle;
            let held = self.host.desk.monitors().cycling.pause_typing && self.cycling_typing();
            if tool(&mut row, p, "chevron-left", tr!("folderstorm-cycling-previous"), false, some).clicked() {
                acts.push(act(Act::Step(false)));
            }
            let (icon, tip) = match (cycle.paused(), held) {
                (true, _) => ("player-play-filled", tr!("folderstorm-cycling-resume")),
                (false, true) => ("player-pause-filled", tr!("folderstorm-cycling-typing")),
                (false, false) => ("player-pause-filled", tr!("folderstorm-cycling-pause")),
            };
            if tool(&mut row, p, icon, tip, cycle.paused() || held, true).clicked() {
                acts.push(act(Act::Pause(!cycle.paused())));
            }
            if tool(&mut row, p, "chevron-right", tr!("folderstorm-cycling-next"), false, some).clicked() {
                acts.push(act(Act::Step(true)));
            }
        }
        // Switch: the character shown comes to the front, and this window
        // shows the one that left it
        let can_switch = state.is_some_and(|(slot, state)| state.live() && !state.closing && active != Some(*slot));
        if tool(
            &mut row,
            p,
            "arrows-exchange",
            tr!("folderstorm-monitor-switch-help"),
            false,
            can_switch,
        )
        .clicked()
            && let Some((slot, _)) = state
        {
            acts.push(act(Act::Switch(seat, *slot)));
        }
        let chat_tip = if config.chat {
            tr!("folderstorm-monitor-chat-hide")
        } else {
            tr!("folderstorm-monitor-chat-show")
        };
        if tool(&mut row, p, "message-circle-filled", chat_tip, config.chat, true).clicked() {
            acts.push(act(Act::Chat(seat, !config.chat)));
        }
        // pinned: an upright pin on the accent; not pinned: a plain,
        // tilted one (as the "Character controls" window's)
        let (pin_icon, pin_tip) = if config.pinned {
            ("pinned-filled", tr!("folderstorm-bar-pinned-help"))
        } else {
            ("pin-filled", tr!("folderstorm-bar-pin-help"))
        };
        if tool(&mut row, p, pin_icon, pin_tip, config.pinned, true).clicked() {
            acts.push(act(Act::Pin(seat, !config.pinned)));
        }
        if tool(&mut row, p, "settings-filled", tr!("folderstorm-monitor-options"), options, true).clicked() {
            acts.push(act(Act::Options(seat, !options)));
        }
    }

    /// How a seat's monitor does with that character, in words, and
    /// whether it is something to notice: the picture's age, then its
    /// viewer's state.
    fn monitor_status(&self, seat: Seat, slot: usize, state: &Slot, now: Instant) -> (String, bool) {
        let shown = self.host.screens[slot].shown;
        let watch = self.seat_watch(seat, slot);
        let viewer = state.status.as_ref().map(|status| status.monitor);
        let health = self.host.desk.health(slot, now);
        let (text, notice) = super::monitors::state_text(watch, viewer, health, shown.is_some());
        let line = match shown {
            Some(shown) if text.is_empty() => super::monitors::age_text(now.saturating_duration_since(shown.at)),
            Some(shown) => format!("{} · {text}", super::monitors::age_text(now.saturating_duration_since(shown.at))),
            None => text,
        };
        (line, notice)
    }

    fn seat_watch(&self, seat: Seat, slot: usize) -> Watch {
        match seat {
            Seat::Slot(_) => self.host.desk.watch(slot),
            Seat::Cycling => self.host.desk.cycling_watch(slot),
        }
    }

    /// The picture, as large as its room takes with its proportions; why
    /// there is none, in its place. It senses neither clicks nor keys.
    fn monitor_picture_in(&mut self, ui: &mut egui::Ui, p: &Palette, room: Rect, seat: Seat, state: Option<&(usize, Slot)>, now: Instant) {
        ui.painter().rect_filled(room, 0.0, p.navy);
        let reason = |ui: &egui::Ui, text: &str| {
            let galley = ui.painter().layout(
                text.to_owned(),
                egui::FontId::proportional(13.0),
                p.muted,
                (room.width() - 24.0).max(40.0),
            );
            ui.painter().galley(room.center() - galley.size() / 2.0, galley, p.muted);
        };
        let Some((slot, state)) = state else {
            // (the cycling monitor, with nobody behind the active one)
            reason(ui, tr!("folderstorm-cycling-nobody"));
            return;
        };
        let slot = *slot;
        let screen = &self.host.screens[slot];
        let (serial, picture, shown) = (screen.serial, screen.picture.clone(), screen.shown);
        let visible = room.height() >= 8.0 && ui.is_rect_visible(room);
        let Some(view) = self.view_mut(seat.which()) else {
            return;
        };
        view.monitors.seen[slot] = visible;
        view.monitors.show(ui.ctx(), slot, serial, picture);
        let texture = view.monitors.texture(slot).map(egui::TextureHandle::id);
        let (status, _) = self.monitor_status(seat, slot, state, now);
        match (texture, shown) {
            (Some(texture), Some(shown)) => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                let at = super::monitors::fitted(room, shown.size);
                ui.painter().image(texture, at, uv, egui::Color32::WHITE);
            }
            _ => reason(ui, &status),
        }
        // (hover only: no click and no key goes anywhere from here)
        if !status.is_empty() {
            ui.interact(
                room,
                ui.scope_id().with(("folderstorm-monitor-picture", seat)),
                egui::Sense::hover(),
            )
            .on_hover_text(status);
        }
    }

    /// The chat part: the character's conversations as chips, the lines
    /// of the one chosen (or the "+N" list), and the box that speaks as
    /// that character. Returns what it shows, for the unread counts.
    #[allow(clippy::too_many_arguments)]
    fn monitor_chat(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        room: Rect,
        seat: Seat,
        state: Option<&(usize, Slot)>,
        now: Instant,
        acts: &mut Vec<HostAct>,
    ) -> Viewing {
        let Some((slot, state)) = state else {
            self.monitors.pane_mut(seat).typing = false;
            return Viewing::Nothing;
        };
        let slot = *slot;
        // (under the header: the most a link's question may cover)
        let below_header =
            Rect::from_min_max(egui::pos2(room.left(), ui.max_rect().top() + HEADER as f32), room.max).intersect(ui.clip_rect());
        let mut ui = child(ui, room, Vec2::new(6.0, 4.0));
        let name = faces::shown_name(state);
        let pane = self.monitors.pane(seat);
        let (mut shown, mut more) = (pane.shown[slot], pane.more);
        // (checkpoint 11: a sheet is that character's; the cycling
        // monitor's next turn closes it)
        let mut sheet = pane.sheet.clone().filter(|(of, _)| *of == slot).map(|(_, sheet)| sheet);
        let mut link = pane.link.clone();
        let chat = &mut self.host.chat;
        if chat.open_thread(slot, shown).is_none() {
            shown = chat.open_threads(slot).next().map(|t| t.conversation).unwrap_or_default();
        }
        let (drawn, sheet_changed) =
            contacts_ui::chips_and_buttons(&mut ui, p, |ui| chips::row(ui, p, chat, slot, Some(shown), more), &mut sheet, &name);
        let mut acted = sheet_changed;
        if sheet_changed {
            more = false;
        }
        let was = (shown, more);
        acted |= drawn.asked.apply(chat, slot, &mut shown, &mut more);
        if (shown, more) != was {
            sheet = None;
        }
        more &= !drawn.hidden.is_empty();
        ui.add_space(3.0);
        let book = &self.host.book;
        let key = seat.look_key();
        if contacts_ui::notice_row(&mut ui, p, book, key, &|_| name.clone()) {
            self.host.book.dismiss(key);
            acted = true;
        }
        let chat = &mut self.host.chat;
        let book = &self.host.book;
        let in_world = chat.login(slot).is_some();
        let mut answer = false;
        let mut open = None;
        let mut web = None;
        // what goes under the lines keeps its whole room: a link's
        // question (in the box's place: a monitor's window is small), or
        // the box, measured first; the sheet takes the box's room too
        // (nothing is typed there)
        let asked_before = link.is_some();
        let speaks = sheet.is_none() && chat.open_thread(slot, shown).is_some();
        let who = chat_ui::Speaker {
            slot,
            conversation: shown,
            name: &name,
            said_as: None,
        };
        let question = match &link {
            Some(ask) => chat_ui::measure(&mut ui, "folderstorm-monitor-link-probe", |ui| {
                chat_ui::link_question(ui, p, ask);
            }),
            None => 0.0,
        };
        let under = match &link {
            Some(_) => question + 3.0,
            None if speaks => {
                let height = chat_ui::measure(&mut ui, "folderstorm-monitor-box-probe", |ui| {
                    chat_ui::compose(ui, p, chat, who, now);
                });
                (height + 3.0).max(BOX_ROOM)
            }
            None => 0.0,
        };
        let left = ui.available_height() - under;
        // (a question comes first: too little room for lines, no lines)
        let lines = if asked_before { left } else { left.max(24.0) };
        let lined = lines >= 24.0;
        if lined {
            egui::Frame::new()
                .fill(p.field)
                .inner_margin(6.0)
                .corner_radius(ui.visuals().window_corner_radius)
                .show(&mut ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt((
                            "folderstorm-monitor-lines",
                            seat,
                            slot,
                            shown,
                            more,
                            sheet.as_ref().map(|s| s.which),
                        ))
                        .auto_shrink([false, false])
                        .max_height((lines - 12.0).max(12.0))
                        .stick_to_bottom(!more && sheet.is_none())
                        .show(ui, |ui| {
                            if let Some(open_sheet) = sheet.as_mut() {
                                let clicks = contacts_ui::sheet(ui, p, book, slot, &name, in_world, open_sheet);
                                if clicks.close {
                                    sheet = None;
                                    acted = true;
                                }
                                open = clicks.open;
                                return;
                            }
                            if more {
                                let clicks = chips::list(ui, p, chat, slot, &drawn.hidden);
                                acted |= clicks.apply(chat, slot, &mut shown, &mut more);
                                return;
                            }
                            match chat.open_thread(slot, shown) {
                                Some(thread) => {
                                    if thread.earlier_login {
                                        ui.label(RichText::new(tr!("folderstorm-chat-earlier-login")).italics().color(p.muted));
                                    }
                                    for entry in thread.entries().filter(|entry| chat.shows_entry(entry)) {
                                        match chat_ui::entry_row(ui, p, entry, None) {
                                            chat_ui::Clicked::Answer | chat_ui::Clicked::Grid => answer = true,
                                            chat_ui::Clicked::Web(target) => web = Some(target),
                                            chat_ui::Clicked::Nothing | chat_ui::Clicked::Place => {}
                                        }
                                    }
                                }
                                None => {
                                    ui.label(RichText::new(tr!("folderstorm-chat-empty", name = name.clone())).color(p.muted));
                                }
                            }
                        });
                });
        }
        ui.add_space(3.0);
        if let Some(target) = web {
            link = chat_ui::follow_web(ui.ctx(), &target);
            // (a question asked now has no room yet: it is drawn in the
            // next frame, which this asks for)
            acted = true;
        }
        let mut composed = chat_ui::Composed::default();
        #[cfg(test)]
        let mut asked_at = None;
        if let Some(ask) = &link {
            if asked_before {
                // at the bottom of the window, where the lines left it its
                // room; in a window too low for that, over what is above
                // it (the chips, the picture), never past the header
                let bottom = room.bottom() - 4.0;
                let top = (bottom - question).max(below_header.top());
                let at = Rect::from_min_max(egui::pos2(room.left() + 6.0, top), egui::pos2(room.right() - 6.0, bottom));
                let mut place = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("folderstorm-monitor-link")
                        .max_rect(at)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                place.set_clip_rect(below_header);
                let answered = chat_ui::link_question(&mut place, p, ask);
                #[cfg(test)]
                {
                    asked_at = Some(place.min_rect());
                }
                if answered {
                    link = None;
                    acted = true;
                }
            }
        } else if speaks {
            composed = chat_ui::compose(&mut ui, p, chat, who, now);
        }
        let viewing = match chat.open_thread(slot, shown) {
            Some(_) if lined && !more && sheet.is_none() => Viewing::Thread(slot, shown),
            _ => Viewing::Nothing,
        };
        if let Some((what, who)) = open {
            // asked of that character's viewer, which decides; a profile is
            // in its own window: the character comes forward
            match self.host.book.open(slot, self.host.chat.login(slot), key, what, &who, now) {
                Ok(_) if contacts_ui::wants_switch(what) => answer = true,
                Ok(_) => {}
                Err(why) => log::info!("session host: nothing asked of character {} ({why:?})", slot + 1),
            }
            acted = true;
        }
        let pane = self.monitors.pane_mut(seat);
        pane.shown[slot] = shown;
        pane.more = more;
        pane.typing = composed.focused;
        pane.sheet = sheet.map(|sheet| (slot, sheet));
        pane.link = link;
        #[cfg(test)]
        {
            pane.asked_at = asked_at;
        }
        if seat == Seat::Cycling && composed.typed {
            // a text typed here: the turn waits for it
            self.monitors.typed = Some((slot, shown));
        }
        if answer {
            acts.push(HostAct::Monitors(Act::Switch(seat, slot)));
        } else if acted || composed.acted {
            acts.push(HostAct::Refresh);
        }
        viewing
    }

    /// A size's and a rate's choices, for a seat's options and for the
    /// monitors' page. `salt`: what tells two such rows apart.
    fn spec_choices(&self, ui: &mut egui::Ui, p: &Palette, spec: Spec, salt: &str) -> Option<Spec> {
        let mut chosen_spec = None;
        ui.push_id(("folderstorm-monitor-size", salt), |ui| {
            let sizes: Vec<String> = Size::ALL.iter().map(|size| size_text(*size)).collect();
            let options: Vec<(&str, bool)> = sizes.iter().map(|text| (text.as_str(), true)).collect();
            let mut chosen = Size::ALL.iter().position(|size| *size == spec.size).unwrap_or(0);
            if widgets::segmented(ui, p, &mut chosen, &options) {
                let size = Size::ALL[chosen.min(Size::ALL.len() - 1)];
                chosen_spec = Some(Spec { size, ..spec });
            }
        });
        ui.add_space(4.0);
        ui.push_id(("folderstorm-monitor-rate", salt), |ui| {
            let rates: Vec<String> = Rate::ALL
                .iter()
                .map(|rate| tr!("folderstorm-monitor-rate", rate = super::monitors::rate_text(*rate)))
                .collect();
            let options: Vec<(&str, bool)> = rates.iter().map(|text| (text.as_str(), true)).collect();
            let mut chosen = Rate::ALL.iter().position(|rate| *rate == spec.rate).unwrap_or(0);
            if widgets::segmented(ui, p, &mut chosen, &options) {
                let rate = Rate::ALL[chosen.min(Rate::ALL.len() - 1)];
                chosen_spec = Some(Spec { rate, ..spec });
            }
        });
        chosen_spec
    }

    /// The cycling monitor's turn and "Pause cycling while typing".
    fn cycling_choices(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let cycling = self.host.desk.monitors().cycling;
        ui.label(RichText::new(tr!("folderstorm-cycling-interval")).color(p.ink));
        ui.add_space(2.0);
        ui.push_id("folderstorm-cycling-interval", |ui| {
            // the turn chosen is among the choices, whatever the file said
            let mut seconds: Vec<u32> = INTERVALS_S.to_vec();
            if !seconds.contains(&cycling.interval_s) {
                seconds.push(cycling.interval_s);
                seconds.sort_unstable();
            }
            let texts: Vec<String> = seconds.iter().map(|seconds| interval_text(*seconds)).collect();
            let options: Vec<(&str, bool)> = texts.iter().map(|text| (text.as_str(), true)).collect();
            let mut chosen = seconds.iter().position(|s| *s == cycling.interval_s).unwrap_or(0);
            if widgets::segmented(ui, p, &mut chosen, &options) {
                acts.push(HostAct::Monitors(Act::Interval(seconds[chosen.min(seconds.len() - 1)])));
            }
        });
        ui.add_space(4.0);
        let mut pause = cycling.pause_typing;
        if widgets::checkbox(ui, p, &mut pause, tr!("folderstorm-cycling-pause-typing"))
            .on_hover_text(tr!("folderstorm-cycling-pause-typing-help"))
            .changed()
        {
            acts.push(HostAct::Monitors(Act::PauseTyping(pause)));
        }
    }

    /// The note on the budget all monitors share, and what this seat is
    /// granted when that is less than chosen.
    fn budget_note(&self, ui: &mut egui::Ui, p: &Palette, seat: Option<Seat>) {
        ui.label(RichText::new(tr!("folderstorm-monitor-budget-note")).color(p.muted));
        let lowered = seat
            .and_then(|seat| self.seat_slot(seat).map(|slot| self.seat_watch(seat, slot)))
            .and_then(|watch| match watch {
                Watch::Asked { order, lowered: true } => Some(order.interval_ms),
                _ => None,
            });
        if let Some(interval_ms) = lowered {
            let rate = super::monitors::per_second(interval_ms);
            ui.label(RichText::new(tr!("folderstorm-monitor-budget", rate = rate)).color(p.amber));
        }
    }

    /// The options of a monitor's window, in the window: its size and
    /// rate, the budget's note, "Show chat", the cycling monitor's turn,
    /// and "Use this size and rate as monitor defaults".
    fn monitor_options(&mut self, ui: &mut egui::Ui, p: &Palette, room: Rect, seat: Seat, config: rules::Window, acts: &mut Vec<HostAct>) {
        let mut ui = child(ui, room, Vec2::new(8.0, 6.0));
        let spec = self.seat_spec(seat);
        let defaults = self.host.desk.monitors().defaults;
        egui::ScrollArea::vertical()
            .id_salt(("folderstorm-monitor-options", seat))
            .auto_shrink([false, false])
            .show(&mut ui, |ui| {
                if let Some(spec) = self.spec_choices(ui, p, spec, "options") {
                    acts.push(HostAct::Monitors(Act::Spec(seat, spec)));
                }
                ui.add_space(4.0);
                self.budget_note(ui, p, Some(seat));
                ui.add_space(6.0);
                let mut chat = config.chat;
                if widgets::checkbox(ui, p, &mut chat, tr!("folderstorm-monitor-show-chat")).changed() {
                    acts.push(HostAct::Monitors(Act::Chat(seat, chat)));
                }
                if seat == Seat::Cycling {
                    ui.add_space(6.0);
                    self.cycling_choices(ui, p, acts);
                }
                ui.add_space(6.0);
                let same = defaults == spec;
                let label = tr!("folderstorm-monitor-use-as-defaults");
                let tip = if same {
                    tr!("folderstorm-monitor-are-defaults")
                } else {
                    tr!("folderstorm-monitor-use-as-defaults-help")
                };
                if widgets::action_button_if(ui, p, "check", label, ActionKind::Flat, BUTTON, !same)
                    .on_hover_text(tip)
                    .clicked()
                {
                    acts.push(HostAct::Monitors(Act::Defaults(spec)));
                }
                ui.add_space(4.0);
                if widgets::action_button(ui, p, "arrow-left", tr!("common-back"), ActionKind::Flat, BUTTON).clicked() {
                    acts.push(HostAct::Monitors(Act::Options(seat, false)));
                }
            });
    }

    /// Checkpoint 10: the monitors' section of the settings window (it was
    /// checkpoint 9's page): a new monitor's size and rate, the budget's
    /// note, the cycling monitor.
    pub(super) fn monitors_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let monitors = *self.host.desk.monitors();
        ui.label(RichText::new(tr!("folderstorm-monitors-page-defaults")).strong().color(p.ink));
        ui.add_space(4.0);
        if let Some(spec) = self.spec_choices(ui, p, monitors.defaults, "defaults") {
            acts.push(HostAct::Monitors(Act::Defaults(spec)));
        }
        ui.add_space(4.0);
        self.budget_note(ui, p, None);
        ui.add_space(12.0);
        ui.label(RichText::new(tr!("folderstorm-bar-monitors-cycling")).strong().color(p.ink));
        ui.label(RichText::new(tr!("folderstorm-cycling-intro")).color(p.muted));
        ui.add_space(4.0);
        let mut open = monitors.cycling.open;
        if widgets::checkbox(ui, p, &mut open, tr!("folderstorm-cycling-open")).changed() {
            acts.push(HostAct::Monitors(Act::Cycling(open)));
        }
        ui.add_space(4.0);
        self.cycling_choices(ui, p, acts);
        ui.add_space(6.0);
        ui.label(RichText::new(tr!("folderstorm-cycling-spec")).color(p.ink));
        ui.add_space(2.0);
        if let Some(spec) = self.spec_choices(ui, p, monitors.cycling.spec, "cycling") {
            acts.push(HostAct::Monitors(Act::Spec(Seat::Cycling, spec)));
        }
    }
}

#[cfg(test)]
mod tests;
