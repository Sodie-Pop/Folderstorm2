//! The window's side of checkpoint 8, the bar (session-host plan, section
//! 15). The bar's rules are the `folderstorm-session` crate's (`bar`, the
//! desk's `bar_state` and `bar_request`), its drawing is
//! `folderstorm_characters/bar.rs`, shared with the viewers. This file is
//! what the host's windows do for it:
//!
//! - **The clicks.** A click on a viewer's bar comes up its link
//!   (`Host::bar_asked`); a click on the host's own compact window comes
//!   from its drawing. Both go through [`Window_::bar_apply`], which does
//!   what the same click does in the host's own window and nothing more.
//! - **The compact window** ("Character controls"): the bar popped out,
//!   one row, with a pin (always on top) and "Dock". Its place and its
//!   pin are remembered. Closing it docks the bar back: it never closes
//!   the host or a character. A window that cannot be made leaves the bar
//!   docked. Its menus and hovers are drawn in the window, as egui's are:
//!   while one is open the window grows under the row to hold it, and
//!   goes back to one row when it closes ([`wanted_height`]).
//! - **The host's own window hides** while a bar is on screen to bring it
//!   back (`bar::host_hidden`), once the user has left it: never while
//!   they are in it, never when it was asked for from the bar (until they
//!   close it), never when no bar is left. A hidden host loses nothing:
//!   its loop does not run on its window being drawn.
//! - **Its page** in the characters' panel ("Bar and quick actions"):
//!   where the bar is, the two label modes, the twelve quick actions, the
//!   two sound and voice rules, the taskbar's and the hiding's choices.
//!
//! No window made or shown here takes the keyboard by itself: only what
//! the user asked for from a bar comes forward.

use super::comfort::Page;
use super::panels::{View, Which};
use super::{Act, BUTTON, Window_};
use crate::folderstorm_characters::bar::{self as drawn, Home};
use crate::theme::Palette;
use crate::ui::{
    menu,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::bar::{self, LabelMode, Placement, QuickAction, Request, WINDOW_MIN_SIZE, WINDOW_SIZE};
use ::folderstorm_session::panels::{self, Panel, Place};
use aurora_i18n::tr;
use egui::RichText;
use std::time::Instant;
use winit::{
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowLevel},
};

/// What the window keeps for the bar.
#[derive(Default)]
pub(super) struct Bar {
    /// The compact window, while the bar is popped out.
    pub view: Option<View>,
    /// The compact window found popped out when the host started has its
    /// window (it opens without taking the keyboard).
    restored: bool,
    /// The pin the compact window was last given.
    pinned: bool,
    /// The user asked for the host's own window from a bar: it stays until
    /// they close it.
    pub asked: bool,
    /// The user closed the host's own window while a bar can bring it
    /// back: hidden at once.
    dismissed: bool,
    /// The host's own window is hidden.
    pub hidden: bool,
    /// Windows: the host's own window is off the taskbar.
    off_taskbar: bool,
    /// The window to bring forward at the next wait.
    raise: Option<Which>,
    /// The compact window's height last asked for, in pixels, while it is
    /// grown to hold a menu or a hover: none at one row.
    grown: Option<u32>,
}

/// The compact window's row, in points (its height when nothing is open).
const ROW: f32 = WINDOW_SIZE[1] as f32;
/// The most the compact window grows under its row for a menu or a hover.
const MENU_ROOM: f32 = 420.0;
/// What is left under a menu or a hover once it is measured.
const MENU_MARGIN: f32 = 6.0;

/// The height, in points, the compact window wants for what its frame
/// showed, `current` high: one row, or with a menu or a hover open, room
/// under the row for it. egui keeps an open menu inside its window, so one
/// that touches the window's bottom may be cut and its size is not known:
/// the window then grows to the most it may, and the next frame, where the
/// menu has its room, measures it and fits it.
pub(super) fn wanted_height(ctx: &egui::Context, current: f32) -> f32 {
    let bottom = ctx.memory(|memory| {
        memory
            .areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|layer| matches!(layer.order, egui::Order::Foreground | egui::Order::Tooltip))
            .filter_map(|layer| memory.area_rect(layer.id))
            .map(|rect| rect.bottom())
            .reduce(f32::max)
    });
    let most = ROW + MENU_ROOM;
    match bottom {
        None => ROW,
        Some(bottom) if bottom + MENU_MARGIN > current + 0.5 => most,
        Some(bottom) => (bottom + MENU_MARGIN).clamp(ROW, most),
    }
}

/// A change of the bar's settings, from its page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edit {
    Quick(usize, QuickAction),
    QuickLabels(LabelMode),
    ToolbarLabels(LabelMode),
    OneTaskbarButton(bool),
    HideHost(bool),
}

/// A label mode's name.
fn mode_name(mode: LabelMode) -> &'static str {
    match mode {
        LabelMode::IconOnly => tr!("folderstorm-bar-labels-icons"),
        LabelMode::TextOnly => tr!("folderstorm-bar-labels-text"),
        LabelMode::IconAndText => tr!("folderstorm-bar-labels-both"),
    }
}

/// A quick action's icon in its list.
fn action_icon(action: QuickAction) -> &'static str {
    match action {
        QuickAction::Unused => "x",
        QuickAction::Character(_) => "user-filled",
        QuickAction::Attention => "bell-filled",
        QuickAction::MuteBackground => "volume-off",
        QuickAction::VoiceFollows => "microphone-filled",
    }
}

impl Window_ {
    /// A quick action's name; a character's says who it is today.
    fn action_name(&self, action: QuickAction) -> String {
        match action {
            QuickAction::Unused => tr!("folderstorm-bar-quick-unused").to_owned(),
            QuickAction::Character(slot) => {
                let number = tr!("folderstorm-hotkeys-character", number = slot + 1);
                match self.host.desk.slot(usize::from(slot)) {
                    Some(state) => format!("{number} ({})", super::shown_name(state)),
                    None => number,
                }
            }
            QuickAction::Attention => tr!("folderstorm-bar-attention").to_owned(),
            QuickAction::MuteBackground => tr!("folderstorm-bar-mute-long").to_owned(),
            QuickAction::VoiceFollows => tr!("folderstorm-bar-voice-long").to_owned(),
        }
    }

    /// A click on the bar, a viewer's or the compact window's: what the
    /// desk does for it, then what only the host's windows can.
    pub(super) fn bar_apply(&mut self, request: Request, now: Instant) {
        if !request.valid() {
            return;
        }
        log::info!("session host: bar, {request:?}");
        self.host.desk.bar_request(request, now);
        match request {
            Request::Show(page) => self.show_host(page),
            // (their windows open at the next wait, in front: asked for)
            Request::Chat(true) => self.bar.raise = Some(Which::Pane(Panel::Chat)),
            Request::Inbox => self.bar.raise = Some(Which::Pane(Panel::Inbox)),
            // (checkpoint 9: each monitor's own window)
            Request::Monitor(slot, true) => self.bar.raise = Some(Which::Monitor(usize::from(slot))),
            Request::Cycling(true) => self.bar.raise = Some(Which::Cycling),
            Request::Dock(false) => self.bar.raise = Some(Which::Bar),
            _ => {}
        }
    }

    /// The host's own window comes forward on that page.
    fn show_host(&mut self, page: bar::Page) {
        self.bar.asked = true;
        self.bar.dismissed = false;
        // the pages are the characters' panel's: in the window that has it
        let mut raise = self.home(Panel::Controls);
        match page {
            bar::Page::Bar => self.comfort.page = Page::Bar,
            bar::Page::Hotkeys => self.comfort.page = Page::Hotkeys,
            // (checkpoint 9: the monitors have a page of their own)
            bar::Page::Monitors => self.comfort.page = Page::Monitors,
            bar::Page::Host => raise = Which::Main,
            bar::Page::Quit => {
                raise = Which::Main;
                // (the question is asked under the characters, not under
                // one of their pages)
                self.comfort.page = Page::Characters;
                if self.host.desk.live() == 0 {
                    // nothing runs: nothing to ask
                    self.host.exit = true;
                } else {
                    // the usual question, in the host's own window
                    self.closing = true;
                    self.quit_asked = false;
                }
            }
        }
        // (a narrow window shows one page: the characters')
        self.chat_page = false;
        self.bar.raise = Some(raise);
    }

    /// The clicks on the viewers' bars since the last look; true when
    /// there was one.
    pub(super) fn bar_pump(&mut self, now: Instant) -> bool {
        let asked = std::mem::take(&mut self.host.bar_asked);
        for request in &asked {
            self.bar_apply(*request, now);
        }
        !asked.is_empty()
    }

    /// Whether the host's own window may hide now: a bar is on screen to
    /// bring it back.
    pub(super) fn host_may_hide(&self, asked: bool) -> bool {
        let desk = &self.host.desk;
        let config = desk.bar();
        // (a compact window that could not be made brings nothing back)
        let there = config.placement == Placement::Docked || self.bar.view.is_some();
        there && bar::host_hidden(config.hide_host, asked, config.placement, desk.bars_shown(), desk.slots().count())
    }

    /// The host's own window was closed while a bar can bring it back: it
    /// hides, nothing is asked and nothing ends. False: the close button
    /// is the host's as before.
    pub(super) fn bar_closes_host(&mut self) -> bool {
        if !self.host_may_hide(false) {
            return false;
        }
        self.bar.asked = false;
        self.bar.dismissed = true;
        self.closing = false;
        self.quit_asked = false;
        true
    }

    /// The host's windows follow the bar: the compact window opens or
    /// closes, takes its pin, the host's own window hides or shows, and
    /// what the user asked for comes forward.
    pub(super) fn sync_bar(&mut self, event_loop: &ActiveEventLoop) {
        let restoring = !std::mem::replace(&mut self.bar.restored, true);
        let popped = self.host.desk.bar().placement == Placement::Popped;
        match (popped, self.bar.view.is_some()) {
            (true, false) => match self.open_bar(event_loop, !restoring) {
                Ok(view) => {
                    log::info!("session host: the bar in its own window");
                    self.bar.pinned = self.host.desk.bar().pinned;
                    self.bar.grown = None;
                    self.bar.view = Some(view);
                    self.redraw_all();
                }
                Err(e) => {
                    // no window: the bar stays in the viewers
                    log::warn!("session host: no window for the bar ({e}): docked");
                    self.host.desk.edit_bar(|bar| bar.placement = Placement::Docked);
                }
            },
            (false, true) => {
                log::info!("session host: the bar docked back");
                if let Some(mut view) = self.bar.view.take() {
                    view.detach();
                }
                self.redraw_all();
            }
            _ => {}
        }
        let pinned = self.host.desk.bar().pinned;
        if let Some(window) = self.bar.view.as_ref().and_then(|view| view.window.as_ref())
            && std::mem::replace(&mut self.bar.pinned, pinned) != pinned
        {
            window.set_window_level(level(pinned));
        }
        self.sync_host_window();
        if let Some(which) = self.bar.raise.take()
            && let Some(window) = self.view(which).and_then(|view| view.window.as_ref())
        {
            // asked for by a click: restored and in front (the viewer
            // that was clicked let this process take the foreground)
            window.set_minimized(false);
            crate::folderstorm_characters::foreground::bring_forward(window);
        }
    }

    /// The host's own window hides or shows.
    fn sync_host_window(&mut self) {
        if !self.host_may_hide(false) {
            // no bar to bring it back: it is there, as a host's window is
            self.bar.asked = false;
            self.bar.dismissed = false;
        }
        // never while the user is in it, unless they closed it
        let hidden = self.host_may_hide(self.bar.asked) && (self.bar.dismissed || !self.main.focused);
        if std::mem::replace(&mut self.bar.hidden, hidden) != hidden {
            log::info!(
                "session host: its own window {}",
                if hidden { "hides: the bar brings it back" } else { "shows" }
            );
            self.main.hidden = hidden;
            if let Some(window) = &self.main.window {
                window.set_visible(!hidden);
            }
            if !hidden {
                self.main.redraw = true;
            }
        }
        // Windows, one taskbar button: while the bar is docked in a
        // viewer, the host's window shown from it is the bar's, and keeps
        // off the taskbar; back on it as soon as no bar is docked
        let config = self.host.desk.bar();
        let off = config.one_taskbar_button && config.placement == Placement::Docked && self.host.desk.bars_shown() > 0;
        if std::mem::replace(&mut self.bar.off_taskbar, off) != off
            && let Some(window) = &self.main.window
        {
            skip_taskbar(window, off);
        }
    }

    /// The compact window's title.
    fn bar_title(&self) -> String {
        format!("{} — {}", tr!("folderstorm-bar-title"), self.title)
    }

    fn open_bar(&self, event_loop: &ActiveEventLoop, active: bool) -> anyhow::Result<View> {
        use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
        let config = self.host.desk.bar();
        // one row: as wide as the user likes, never higher
        let mut attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(self.bar_title())
            .with_inner_size(LogicalSize::new(WINDOW_SIZE[0], WINDOW_SIZE[1]))
            .with_min_inner_size(LogicalSize::new(WINDOW_MIN_SIZE[0], WINDOW_MIN_SIZE[1]))
            .with_max_inner_size(LogicalSize::new(16_384, WINDOW_SIZE[1]))
            .with_window_level(level(config.pinned))
            .with_active(active);
        let place = config
            .place
            .map(|saved| panels::on_screen(saved, &super::panels::screens(event_loop)));
        if let Some([x, y]) = place.and_then(|place| place.position) {
            attributes = attributes.with_position(PhysicalPosition::new(x, y));
        }
        let window = event_loop.create_window(attributes)?;
        if let Some(place) = place {
            // its width as it was; its height is the row's on this screen
            let _ = window.request_inner_size(PhysicalSize::new(place.size[0], window.inner_size().height));
        }
        let mut view = self.look.view();
        view.attach(event_loop, window)?;
        Ok(view)
    }

    /// What the compact window is told, before the interface hears it.
    /// True when the window is gone (closed: the bar is docked back).
    pub(super) fn bar_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => {
                // docked back; never the host's or a character's end
                self.host.desk.edit_bar(|bar| bar.placement = Placement::Docked);
                if let Some(mut view) = self.bar.view.take() {
                    view.detach();
                }
                log::info!("session host: the bar's window closed: docked back");
                self.redraw_all();
                true
            }
            WindowEvent::Focused(false) => {
                // a menu left open in a window the user left would keep
                // it grown: menus close with the keyboard's going
                if let Some(view) = &mut self.bar.view {
                    egui::Popup::close_all(&view.context);
                    view.redraw = true;
                }
                false
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(mut place) = self.bar.view.as_ref().and_then(place) {
                    if self.bar.grown.is_some()
                        && let Some(window) = self.bar.view.as_ref().and_then(|view| view.window.as_ref())
                    {
                        // remembered at one row, never grown for a menu
                        place.size[1] = (ROW * window.scale_factor() as f32).round() as u32;
                    }
                    self.host.desk.edit_bar(|bar| {
                        bar.placed(place);
                    });
                }
                false
            }
            _ => false,
        }
    }

    /// The compact window's content: the row.
    pub(super) fn bar_window(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let p = self.palette;
        let state = self.host.bar_state();
        let home = Home::Host {
            pinned: self.host.desk.bar().pinned,
        };
        // the row at the top; under it, room for a menu while one is open
        let window = ui.max_rect();
        let rect = window.with_max_y(window.top() + ROW.min(window.height()));
        acts.extend(drawn::show_in(ui, &p, rect, &state, home).into_iter().map(Act::Bar));
    }

    /// After a frame of the compact window: it grows under its row for an
    /// open menu or hover, and goes back to one row once it is closed. Its
    /// width, place and pin are left as they are; the place it remembers
    /// is the row's (`bar_event`).
    pub(super) fn bar_fit(&mut self, ctx: &egui::Context, window: &Window) {
        use winit::dpi::{LogicalSize, PhysicalSize};
        let scale = ctx.pixels_per_point().max(0.1);
        let size = window.inner_size();
        let mut wanted = wanted_height(ctx, size.height as f32 / scale);
        if wanted > ROW
            && let (Some(screen), Ok(top)) = (window.current_monitor(), window.inner_position())
        {
            // never past the bottom of its screen
            let below = (screen.position().y + screen.size().height as i32 - top.y) as f32 / scale;
            wanted = wanted.min(below.max(ROW));
        }
        let grown = (wanted > ROW + 0.5).then(|| (wanted * scale).round() as u32);
        if grown == self.bar.grown {
            return;
        }
        let height = grown.unwrap_or_else(|| (ROW * scale).round() as u32);
        if grown.is_some() {
            window.set_max_inner_size(None::<PhysicalSize<u32>>);
        }
        let _ = window.request_inner_size(PhysicalSize::new(size.width, height));
        if grown.is_none() {
            window.set_max_inner_size(Some(LogicalSize::new(16_384, WINDOW_SIZE[1])));
        }
        self.bar.grown = grown;
    }

    pub(super) fn bar_edit(&mut self, edit: Edit) {
        self.host.desk.edit_bar(|bar| match edit {
            Edit::Quick(position, action) => {
                bar.set_quick(position, action);
            }
            Edit::QuickLabels(mode) => bar.quick_labels = mode,
            Edit::ToolbarLabels(mode) => bar.toolbar_labels = mode,
            Edit::OneTaskbarButton(on) => bar.one_taskbar_button = on,
            Edit::HideHost(on) => bar.hide_host = on,
        });
    }

    /// "Bar and quick actions", in place of the characters.
    pub(super) fn bar_page(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<Act>) {
        if widgets::action_button(ui, p, "arrow-left", tr!("common-back"), ActionKind::Flat, BUTTON).clicked() {
            acts.push(Act::Comfort(super::comfort::Act::Page(Page::Characters)));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(tr!("folderstorm-bar-page-heading")).size(18.0).strong().color(p.ink));
        ui.add_space(2.0);
        ui.label(RichText::new(tr!("folderstorm-bar-page-intro")).color(p.muted));
        ui.add_space(10.0);
        let config = self.host.desk.bar().clone();
        egui::ScrollArea::vertical()
            .id_salt("folderstorm-host-bar-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // where the bar is
                ui.label(RichText::new(tr!("folderstorm-bar-page-where")).strong().color(p.ink));
                ui.add_space(4.0);
                let mut place = usize::from(config.placement == Placement::Popped);
                let places = [
                    (tr!("folderstorm-bar-page-docked"), true),
                    (tr!("folderstorm-bar-page-popped"), true),
                ];
                if widgets::segmented(ui, p, &mut place, &places) {
                    acts.push(Act::Bar(Request::Dock(place == 0)));
                }
                ui.add_space(6.0);
                let mut hide = config.hide_host;
                if widgets::checkbox(ui, p, &mut hide, tr!("folderstorm-bar-page-hide-host"))
                    .on_hover_text(tr!("folderstorm-bar-page-hide-host-help"))
                    .changed()
                {
                    acts.push(Act::BarEdit(Edit::HideHost(hide)));
                }
                let mut one = config.one_taskbar_button;
                if widgets::checkbox(ui, p, &mut one, tr!("folderstorm-bar-page-taskbar"))
                    .on_hover_text(tr!("folderstorm-bar-page-taskbar-help"))
                    .changed()
                {
                    acts.push(Act::BarEdit(Edit::OneTaskbarButton(one)));
                }
                if !cfg!(windows) {
                    ui.label(RichText::new(tr!("folderstorm-bar-page-taskbar-windows-only")).color(p.muted));
                }
                ui.add_space(12.0);
                // how the buttons are labelled
                ui.label(RichText::new(tr!("folderstorm-bar-page-labels")).strong().color(p.ink));
                ui.add_space(4.0);
                let modes: Vec<(&str, bool)> = LabelMode::ALL.into_iter().map(|mode| (mode_name(mode), true)).collect();
                for (label, mode, toolbar) in [
                    (tr!("folderstorm-bar-page-toolbar-labels"), config.toolbar_labels, true),
                    (tr!("folderstorm-bar-page-quick-labels"), config.quick_labels, false),
                ] {
                    // (a scope each: the two rows offer the same choices)
                    ui.push_id(("folderstorm-bar-labels", toolbar), |ui| {
                        ui.horizontal(|ui| {
                            row_name(ui, 150.0, RichText::new(label).color(p.ink));
                            let mut selected = mode.index();
                            if widgets::segmented(ui, p, &mut selected, &modes) {
                                let mode = LabelMode::ALL[selected.min(LabelMode::ALL.len() - 1)];
                                acts.push(Act::BarEdit(if toolbar {
                                    Edit::ToolbarLabels(mode)
                                } else {
                                    Edit::QuickLabels(mode)
                                }));
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(8.0);
                // sound and voice
                ui.label(RichText::new(tr!("folderstorm-bar-page-sound")).strong().color(p.ink));
                ui.add_space(4.0);
                let mut mute = self.host.desk.mute_background();
                if widgets::checkbox(ui, p, &mut mute, tr!("folderstorm-bar-mute-long"))
                    .on_hover_text(tr!("folderstorm-bar-mute-off-help"))
                    .changed()
                {
                    acts.push(Act::Bar(Request::MuteBackground(mute)));
                }
                let mut follows = self.host.desk.voice_follows();
                if widgets::checkbox(ui, p, &mut follows, tr!("folderstorm-bar-voice-long"))
                    .on_hover_text(tr!("folderstorm-bar-voice-off-help"))
                    .changed()
                {
                    acts.push(Act::Bar(Request::VoiceFollows(follows)));
                }
                ui.add_space(12.0);
                // the quick actions
                ui.label(RichText::new(tr!("folderstorm-bar-page-quick")).strong().color(p.ink));
                ui.label(RichText::new(tr!("folderstorm-bar-page-quick-intro")).color(p.muted));
                ui.add_space(4.0);
                for (position, held) in config.quick.iter().copied().enumerate() {
                    ui.push_id(("folderstorm-bar-quick", position), |ui| {
                        ui.horizontal(|ui| {
                            let number = RichText::new(tr!("folderstorm-bar-page-position", number = position + 1)).color(p.muted);
                            row_name(ui, 96.0, number);
                            let name = self.action_name(held);
                            let button = widgets::action_menu_button(ui, p, action_icon(held), &name, false, BUTTON.y);
                            menu::dropdown(&button, p, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                                for action in QuickAction::all() {
                                    let name = self.action_name(action);
                                    if menu::check(ui, p, action_icon(action), &name, action == held, true) && action != held {
                                        acts.push(Act::BarEdit(Edit::Quick(position, action)));
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(3.0);
                }
            });
    }
}

/// A row's name before its choice: in a column of that width, from its
/// left edge.
fn row_name(ui: &mut egui::Ui, width: f32, name: RichText) {
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(width, BUTTON.y), layout, |ui| {
        ui.set_min_width(width);
        ui.add(egui::Label::new(name).truncate());
    });
}

/// The compact window's level: over the others when pinned.
fn level(pinned: bool) -> WindowLevel {
    if pinned { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal }
}

/// Where the compact window is, when that is a place to open it again.
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

/// Windows: one of the host's windows off the taskbar, or back on it.
#[cfg(windows)]
fn skip_taskbar(window: &Window, skip: bool) {
    use winit::platform::windows::WindowExtWindows;
    window.set_skip_taskbar(skip);
}

#[cfg(not(windows))]
fn skip_taskbar(_window: &Window, _skip: bool) {}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::tests::{draw, draw_in, draw_with, window};
    use super::*;
    use ::folderstorm_session::host::Effect;
    use ::folderstorm_session::identity::Colour;
    use ::folderstorm_session::ipc::{Screen, Status};

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    /// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps each picture these
    /// tests draw, as `<folder>/<name>.png`, to be looked at.
    fn keep(name: &str, pixels: &[u32], width: u32, height: u32) {
        let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") else {
            return;
        };
        let rgba: Vec<_> = pixels
            .iter()
            .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
            .collect();
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
    }

    fn painted(pixels: &[u32]) -> usize {
        let background = pixels[0];
        pixels.iter().filter(|p| **p != background).count()
    }

    /// Two characters in the world, the first one active, with colours
    /// and an alias; a third one not started.
    fn running(app: &mut Window_, now: Instant) {
        app.host.desk.add("demo-3").expect("a third character");
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
            let status = Status {
                screen: Screen::World,
                name: ["Jane Doe", "Sam Resident"][slot].to_owned(),
                ..Default::default()
            };
            app.host.desk.status(slot, 1, status, now);
        }
        app.host.desk.set_colour(0, Colour::Teal);
        app.host.desk.set_colour(1, Colour::Orange);
        assert_eq!(app.host.desk.set_alias(0, "Main"), Ok(()));
        app.host.desk.take_effects();
    }

    /// The compact window as `sync_bar` makes it, without a window.
    fn pop_out(app: &mut Window_, now: Instant) {
        app.bar_apply(Request::Dock(false), now);
        app.bar.view = Some(app.look.view());
    }

    #[test]
    fn the_compact_window_draws_the_row_on_the_processor_wide_and_narrow() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.demo_bar(None);
        pop_out(&mut app, now);
        // wide: the active character's name, every quick action with its
        // text, the pin and the way back
        draw_in(&mut app, Which::Bar, 1100, 38);
        let (drawn, pixels) = draw_in(&mut app, Which::Bar, 1100, 38);
        assert!(shows(&drawn, "1: Main"), "{drawn:?}");
        assert!(shows(&drawn, "1 · Main") && shows(&drawn, "2 · Sam Resident") && shows(&drawn, "3 · demo-3"));
        assert!(shows(&drawn, tr!("folderstorm-bar-attention")) && shows(&drawn, tr!("folderstorm-bar-mute")));
        assert!(
            !shows(&drawn, tr!("folderstorm-host-intro")),
            "one row, nothing of the host's window"
        );
        assert!(!shows(&drawn, "use of widget"), "two widgets share an id: {drawn:?}");
        assert!(painted(&pixels) > 2_000, "{} pixels", painted(&pixels));
        keep("popped-1100", &pixels, 1100, 38);
        // the characters' colours are under their buttons
        let p = app.palette;
        let teal = crate::folderstorm_characters::host::faces::tint(&p, Colour::Teal).expect("a colour");
        let colour = u32::from_be_bytes([0, teal.r(), teal.g(), teal.b()]);
        assert!(pixels.contains(&colour), "the first character's colour is drawn");
        // narrow: the texts go, the numbers and the icons stay
        let (drawn, pixels) = draw_in(&mut app, Which::Bar, 420, 38);
        assert!(
            !shows(&drawn, "Sam Resident") && !shows(&drawn, tr!("folderstorm-bar-mute")),
            "{drawn:?}"
        );
        assert!(drawn.iter().any(|t| t == "2"), "{drawn:?}");
        assert!(painted(&pixels) > 1_000);
        keep("popped-420", &pixels, 420, 38);
        // narrower than its two ends: still one row; it scrolls as a
        // whole, in its order, and nothing is laid over anything else
        let (drawn, pixels) = draw_in(&mut app, Which::Bar, 200, 38);
        assert!(painted(&pixels) > 500);
        assert!(!shows(&drawn, "use of widget"), "{drawn:?}");
        keep("popped-200", &pixels, 200, 38);
        // pinned: its own picture
        app.apply(vec![Act::Bar(Request::Pin(true))], now);
        let (_, pixels) = draw_in(&mut app, Which::Bar, 1100, 38);
        keep("popped-1100-pinned", &pixels, 1100, 38);
        let accent = u32::from_be_bytes([0, p.violet.r(), p.violet.g(), p.violet.b()]);
        assert!(pixels.contains(&accent), "the pin is on the accent");
    }

    /// The compact window's height for what its last frame showed.
    fn wanted(app: &Window_, current: f32) -> f32 {
        let view = app.view(Which::Bar).expect("the compact window");
        wanted_height(&view.context, current)
    }

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    #[test]
    fn a_menu_of_the_compact_window_grows_it_and_is_painted_whole() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.demo_bar(None);
        pop_out(&mut app, now);
        let (width, row) = (1100, ROW as u32);
        draw_in(&mut app, Which::Bar, width, row);
        draw_in(&mut app, Which::Bar, width, row);
        assert_eq!(wanted(&app, ROW), ROW, "nothing open: one row");
        // the characters' list, opened by a click on its button (the
        // row's first, at its left end)
        let at = egui::pos2(40.0, ROW / 2.0);
        draw_with(&mut app, Which::Bar, width, row, click(at));
        let (drawn, _) = draw_in(&mut app, Which::Bar, width, row);
        assert!(shows(&drawn, "2: Sam Resident"), "the list is open: {drawn:?}");
        // one row high, the list does not fit (this was the bug: egui
        // keeps it in its window and the rest was cut): the window asks
        // for room
        let list = app.view(Which::Bar).and_then(|view| {
            view.context.memory(|memory| {
                memory
                    .areas()
                    .visible_layer_ids()
                    .into_iter()
                    .filter(|layer| layer.order == egui::Order::Foreground)
                    .filter_map(|layer| memory.area_rect(layer.id))
                    .reduce(|a, b| a.union(b))
            })
        });
        assert!(list.is_some_and(|list| list.height() > ROW), "{list:?}");
        let grown = wanted(&app, ROW);
        assert!(grown > ROW + 60.0, "grows for the list: {grown}");
        // grown, the list is measured and the window fitted to it
        let tall = grown.round() as u32;
        draw_in(&mut app, Which::Bar, width, tall);
        let fitted = wanted(&app, grown);
        assert!(fitted > ROW + 60.0 && fitted <= grown, "{fitted} of {grown}");
        let high = fitted.round() as u32;
        let (drawn, pixels) = draw_in(&mut app, Which::Bar, width, high);
        assert!(
            shows(&drawn, "1: Main") && shows(&drawn, "2: Sam Resident") && shows(&drawn, "3: demo-3"),
            "{drawn:?}"
        );
        assert!((wanted(&app, fitted) - fitted).abs() < 1.0, "fitted, it stays");
        // the list is painted under the row, whole: its last line is not
        // at the window's bottom
        let background = pixels[(high as usize - 1) * width as usize];
        let under = &pixels[(row as usize + 4) * width as usize..(high as usize - 2) * width as usize];
        assert!(
            under.iter().filter(|p| **p != background).count() > 2_000,
            "the list is drawn under the row"
        );
        keep("popped-menu", &pixels, width, high);
        // the row stays at the top, as it was
        let (top, _) = draw_in(&mut app, Which::Bar, width, high);
        assert!(shows(&top, tr!("folderstorm-bar-attention")));
        // closed (Escape): one row again
        draw_with(
            &mut app,
            Which::Bar,
            width,
            high,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        draw_in(&mut app, Which::Bar, width, high);
        draw_in(&mut app, Which::Bar, width, high);
        assert_eq!(wanted(&app, fitted), ROW, "closed: one row");
    }

    /// A viewer's bar as its pass draws it: the strip kept by the inset,
    /// the bar in it, and the viewer's own interface under it (here a
    /// plain panel that says where it starts). On the processor, with the
    /// host's theme.
    fn draw_docked(app: &Window_, state: &bar::State, width: u32, height: u32) -> (Vec<String>, Vec<u32>, f32, Vec<Request>) {
        let view = app.look.view();
        let p = app.palette;
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32))),
            safe_area_insets: Some(drawn::inset(Some(state))),
            ..Default::default()
        };
        let mut top = 0.0;
        let mut asked = Vec::new();
        let mut pass = |context: &egui::Context| {
            context.run_ui(raw(), |ui| {
                let ctx = ui.ctx().clone();
                asked = drawn::show_strip(&ctx, &p, state);
                top = ctx.content_rect().top();
                // the viewer's bars, as `App::draw_ui` lays them out: in
                // the root `Ui`, or in the room `bars_room` gives them
                let room = drawn::bars_room(&ctx, None);
                let mut child = room.map(|room| ui.new_child(egui::UiBuilder::new().max_rect(room)));
                let ui = child.as_mut().unwrap_or(ui);
                egui::Panel::top("the-viewer-s-bar")
                    .frame(egui::Frame::new().fill(p.field).inner_margin(8.0))
                    .exact_size(40.0)
                    .show(ui, |ui| ui.label("the viewer's own interface"));
            })
        };
        // (twice: the first pass measures)
        let mut first = pass(&view.context);
        let mut painter = crate::crash::software::Painter::default();
        let mut pixels = vec![0u32; (width * height) as usize];
        let size = [width as usize, height as usize];
        let primitives = view.context.tessellate(std::mem::take(&mut first.shapes), first.pixels_per_point);
        painter.paint(
            &mut pixels,
            size,
            &primitives,
            &first.textures_delta,
            first.pixels_per_point,
            p.panel,
        );
        first.textures_delta.clear();
        let mut output = pass(&view.context);
        fn texts(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, into)),
                _ => {}
            }
        }
        let mut drawn_texts = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut drawn_texts);
        }
        let primitives = view.context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        painter.paint(
            &mut pixels,
            size,
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            p.panel,
        );
        output.textures_delta.clear();
        keep(&format!("docked-{width}-{top}"), &pixels, width, height);
        (drawn_texts, pixels, top, asked)
    }

    #[test]
    fn a_viewer_draws_the_bar_in_a_strip_above_its_own_interface() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.demo_bar(None);
        let state = app.host.bar_state();
        // wide: the row, and the viewer's interface starts under it
        let (drawn, pixels, top, asked) = draw_docked(&app, &state, 1280, 200);
        assert_eq!(top, bar::HEIGHT);
        assert!(asked.is_empty(), "nothing was clicked");
        assert!(shows(&drawn, "1: Main") && shows(&drawn, "2 · Sam Resident"), "{drawn:?}");
        assert!(shows(&drawn, "the viewer's own interface"));
        let p = app.palette;
        let rgb = |c: egui::Color32| u32::from_be_bytes([0, c.r(), c.g(), c.b()]);
        let at = |x: u32, y: u32| pixels[(y * 1280 + x) as usize];
        // the strip's own background at its ends, the viewer's bar under
        // it, whole: its 40 points start where the strip ends, and its
        // text is in sight
        assert_eq!(at(2, 2), rgb(p.panel));
        assert_eq!(at(1277, 27), rgb(p.panel));
        assert_eq!(at(1270, 32), rgb(p.field));
        assert_eq!(at(1270, 68), rgb(p.field));
        assert_ne!(at(1270, 72), rgb(p.field));
        let ink = |rows: std::ops::Range<u32>| {
            rows.flat_map(|y| (0..200).map(move |x| (x, y)))
                .filter(|(x, y)| at(*x, *y) == rgb(p.ink))
                .count()
        };
        assert!(
            ink(30..70) > 20,
            "the viewer's bar is drawn under the strip: {} pixels",
            ink(30..70)
        );
        // narrow: still one row of the same height
        let (drawn, _, top, _) = draw_docked(&app, &state, 420, 200);
        assert_eq!(top, bar::HEIGHT);
        assert!(!shows(&drawn, "Sam Resident"), "{drawn:?}");
        // collapsed: a thin handle, and the interface nearly at the top
        let collapsed = bar::State {
            collapsed: true,
            ..state.clone()
        };
        let (drawn, _, top, _) = draw_docked(&app, &collapsed, 1280, 200);
        assert_eq!(top, bar::HANDLE);
        assert!(!shows(&drawn, "1: Main"), "{drawn:?}");
        // popped out: no strip, nothing of the bar, the whole window
        let popped = bar::State {
            docked: false,
            ..state.clone()
        };
        let (drawn, pixels, top, _) = draw_docked(&app, &popped, 1280, 200);
        assert_eq!(top, 0.0);
        assert!(!shows(&drawn, "1: Main") && shows(&drawn, "the viewer's own interface"));
        assert_eq!(
            pixels[(2 * 1280 + 1270) as usize],
            rgb(p.field),
            "the viewer's bar is at the top, as without a host"
        );
    }

    #[test]
    fn a_click_from_a_bar_does_what_the_host_s_window_does() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        // a viewer's bar asks: its clicks come up the link
        app.host.bar_asked.push(Request::Pick(1));
        app.host.bar_asked.push(Request::MuteBackground(true));
        assert!(app.bar_pump(now));
        assert!(!app.bar_pump(now), "each click once");
        assert_eq!(app.host.desk.active(), Some(1));
        assert!(app.host.desk.mute_background());
        // a character without a viewer is started, once
        app.host.desk.take_effects();
        app.bar_apply(Request::Pick(2), now);
        app.bar_apply(Request::OpenLogin(None), now);
        let spawns = app
            .host
            .desk
            .take_effects()
            .into_iter()
            .filter(|e| matches!(e, Effect::Spawn { .. }))
            .count();
        assert_eq!(spawns, 1);
        // the chat and the inbox get their own windows, asked to the front
        app.bar_apply(Request::Chat(true), now);
        assert!(app.out(Panel::Chat));
        assert_eq!(app.bar.raise, Some(Which::Pane(Panel::Chat)));
        app.bar_apply(Request::Chat(false), now);
        assert!(!app.out(Panel::Chat));
        app.bar_apply(Request::Inbox, now);
        assert!(app.out(Panel::Inbox));
        // a request that names no slot there can be does nothing
        app.host.desk.take_effects();
        app.bar_apply(Request::Pick(9), now);
        assert!(app.host.desk.take_effects().is_empty());
    }

    #[test]
    fn the_bar_s_pages_open_in_the_host_s_window() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.bar_apply(Request::Show(bar::Page::Bar), now);
        assert!(app.bar.asked && app.comfort.page == Page::Bar);
        assert_eq!(app.bar.raise, Some(Which::Main));
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-bar-page-heading")), "{drawn:?}");
        assert!(shows(&drawn, tr!("folderstorm-bar-page-quick")) && shows(&drawn, tr!("folderstorm-bar-labels-icons")));
        assert!(shows(&drawn, "(Main)"), "a character's position says who it is: {drawn:?}");
        assert!(!shows(&drawn, "use of widget"), "two widgets share an id: {drawn:?}");
        assert!(painted(&pixels) > 10_000);
        keep("page", &pixels, 1120, 760);
        // its choices are the desk's
        app.apply(
            vec![
                Act::BarEdit(Edit::Quick(0, QuickAction::Attention)),
                Act::BarEdit(Edit::ToolbarLabels(LabelMode::IconAndText)),
                Act::BarEdit(Edit::QuickLabels(LabelMode::IconOnly)),
                Act::BarEdit(Edit::OneTaskbarButton(false)),
                Act::BarEdit(Edit::HideHost(false)),
                Act::Bar(Request::VoiceFollows(false)),
            ],
            now,
        );
        let config = app.host.desk.bar();
        assert_eq!(config.quick[0], QuickAction::Attention);
        assert_eq!(config.quick[5], QuickAction::Unused, "each action once");
        assert_eq!(
            (config.toolbar_labels, config.quick_labels),
            (LabelMode::IconAndText, LabelMode::IconOnly)
        );
        assert!(!config.one_taskbar_button && !config.hide_host && !app.host.desk.voice_follows());
        // the hotkeys' page, and the monitors' own (checkpoint 9)
        app.bar_apply(Request::Show(bar::Page::Hotkeys), now);
        assert!(app.comfort.page == Page::Hotkeys);
        app.bar_apply(Request::Show(bar::Page::Monitors), now);
        assert!(app.comfort.page == Page::Monitors);
        // "Quit…" asks the usual question in the host's own window
        app.bar_apply(Request::Show(bar::Page::Quit), now);
        assert!(app.closing && !app.host.exit && !app.host.desk.quitting());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-host-close-question")), "{drawn:?}");
    }

    #[test]
    fn the_host_s_window_hides_only_while_a_bar_can_bring_it_back() {
        let now = Instant::now();
        let mut app = window(host());
        // no viewer shows a bar: the host is there, and its close button
        // is the host's
        assert!(!app.host_may_hide(false) && !app.bar_closes_host());
        running(&mut app, now);
        assert!(app.host_may_hide(false));
        // the user is in it: it stays (`window` gives it the keyboard)
        app.sync_host_window();
        assert!(!app.bar.hidden);
        // they went to a character: it hides
        app.main.focused = false;
        app.sync_host_window();
        assert!(app.bar.hidden && app.main.hidden);
        // asked for from the bar: shown, and it stays when they leave it
        app.bar_apply(Request::Show(bar::Page::Host), now);
        app.sync_host_window();
        assert!(!app.bar.hidden && !app.main.hidden);
        app.main.focused = true;
        app.sync_host_window();
        app.main.focused = false;
        app.sync_host_window();
        assert!(!app.bar.hidden, "asked for: there until closed");
        // its close button hides it: nothing is asked, nothing ends
        app.main.focused = true;
        assert!(app.bar_closes_host());
        app.sync_host_window();
        assert!(app.bar.hidden && !app.closing && !app.host.exit);
        assert_eq!(app.host.desk.live(), 2);
        // the setting off: never hidden
        app.bar_edit(Edit::HideHost(false));
        app.main.focused = false;
        app.sync_host_window();
        assert!(!app.bar.hidden);
        assert!(!app.bar_closes_host(), "its close button asks, as before");
        app.bar_edit(Edit::HideHost(true));
        app.sync_host_window();
        assert!(app.bar.hidden);
        // the viewers end: no bar is left, the host is there again
        for slot in 0..2 {
            app.host.desk.exited(slot, 1, true, now);
        }
        app.sync_host_window();
        assert!(!app.bar.hidden && !app.bar.asked);
    }

    #[test]
    fn popped_out_the_compact_window_brings_the_host_back_and_closing_it_docks_the_bar() {
        let now = Instant::now();
        let mut app = window(host());
        running(&mut app, now);
        app.host.desk.take_effects();
        // popped out, but its window could not be made: nothing hides
        app.bar_apply(Request::Dock(false), now);
        assert_eq!(app.bar.raise, Some(Which::Bar));
        assert!(!app.host_may_hide(false));
        app.bar.view = Some(app.look.view());
        assert!(app.host_may_hide(false));
        assert_eq!(app.host.desk.bars_shown(), 0, "no viewer draws it now");
        // the pin is the desk's, saved
        app.apply(vec![Act::Bar(Request::Pin(true))], now);
        assert!(app.host.desk.bar().pinned);
        // moved: remembered
        assert!(!app.bar_event(&WindowEvent::Focused(true)));
        // closed: docked back, the host and the characters stay
        assert!(app.bar_event(&WindowEvent::CloseRequested));
        assert!(app.bar.view.is_none());
        assert_eq!(app.host.desk.bar().placement, Placement::Docked);
        assert!(!app.host.exit && !app.closing);
        assert_eq!(app.host.desk.live(), 2);
        assert_eq!(app.host.desk.bars_shown(), 2);
        // "Dock" from inside the row does the same
        pop_out(&mut app, now);
        app.apply(vec![Act::Bar(Request::Dock(true))], now);
        assert_eq!(app.host.desk.bar().placement, Placement::Docked);
    }

    #[test]
    fn every_label_mode_and_action_has_a_name_and_an_icon() {
        let app = window(host());
        for mode in LabelMode::ALL {
            assert!(!mode_name(mode).is_empty());
        }
        for action in QuickAction::all() {
            assert!(!app.action_name(action).is_empty());
            assert!(crate::ui::icons::embedded(action_icon(action)), "{action:?}");
        }
        assert!(app.action_name(QuickAction::Character(0)).contains("demo-1"));
        assert!(!app.action_name(QuickAction::Character(4)).contains('('), "no character there");
        assert!(crate::ui::icons::embedded("arrow-left"));
    }
}
