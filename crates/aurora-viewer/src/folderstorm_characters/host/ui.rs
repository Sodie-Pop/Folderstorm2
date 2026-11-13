//! The session host's window: one row per character with what its viewer
//! reports and what can be done with it, the stage choice, and "Quit all";
//! beside it (or behind a tab when the window is narrow) the shared chat
//! (`chat_ui.rs`).
//! Drawn on the processor like the crash window (`crash/window.rs`, whose
//! event loop this follows), with the viewer's theme, fonts, icons and
//! catalogues. Checkpoint 4's part (quick actions, hotkeys, "New character
//! profile…") is in `ui/comfort.rs`, checkpoint 5's (the monitors) in
//! `ui/monitors.rs`, checkpoint 6's (the chat, the characters and the
//! monitors each in a window of their own) in `ui/panels.rs`, checkpoint
//! 7's in `ui/inbox.rs` (the attention inbox), `ui/identity.rs` (a
//! character's alias and colour) and `ui/condensed.rs` (one line per
//! character), checkpoint 8's in `ui/bar.rs` (the bar's compact window, its
//! page, and the host's own window hiding while a bar is on screen),
//! checkpoint 9's in `ui/monitor_windows.rs` (each monitor in a window of
//! its own, the cycling monitor, and the monitors' page).

mod bar;
mod comfort;
mod condensed;
mod identity;
mod inbox;
mod monitor_windows;
mod monitors;
mod panels;
mod settings;
// (checkpoint 11: New IM, Friends and links, drawn and clicked)
#[cfg(test)]
mod conversations_tests;

use super::Host;
use super::chat_ui::ChatPanel;
use crate::theme::Palette;
use crate::ui::{
    fonts,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::host::{AddError, Health, Note, Phase, Slot};
use ::folderstorm_session::ipc::{Gpu, MAX_SLOTS, Refusal, Screen, Standby};
use ::folderstorm_session::panels::Panel;
use ::folderstorm_session::profile::{self, NameError};
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use monitor_windows::Seat;
use panels::{Look, View, Which};
use std::{
    num::NonZeroU32,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

/// The window looks at its viewers this often while one runs, and when
/// none does.
const TICK: Duration = Duration::from_millis(200);
const IDLE_TICK: Duration = Duration::from_secs(1);

const BUTTON: Vec2 = Vec2::new(0.0, 26.0);

pub fn run(mut host: Host) -> anyhow::Result<()> {
    // (the global hotkeys are heard by the loop's message hook, and belong
    // to the thread that registers them: this one)
    let mut builder = EventLoop::builder();
    host.comfort.hotkeys = super::hotkeys::Registrar::install(&mut builder);
    host.apply_hotkeys();
    let event_loop = builder.build()?;
    // (checkpoint 8: a click on a viewer's bar is heard at once, not at
    // the window's next look at its viewers)
    let proxy = event_loop.create_proxy();
    super::process::set_waker(move || {
        let _ = proxy.send_event(());
    });
    let settings = crate::settings::Settings::load();
    // (drawn on events and ticks: the system font scan is waited for here)
    fonts::fallback::wait();
    let look = Look {
        theme: crate::ui::skin::recovery_theme(),
        font_scale: settings.font_scale,
        font: Some(settings.font.clone()),
    };
    let mut app = Window_::new(host, look);
    event_loop.run_app(&mut app)?;
    // what was not written yet; the viewers are let go, never ended
    if app.host.unsaved.is_some() {
        app.host.save();
    }
    if let Some(error) = app.error {
        anyhow::bail!(error);
    }
    Ok(())
}

/// The host and its windows: its own, and one for each panel that is out.
struct Window_ {
    host: Host,
    palette: Palette,
    /// What a window's interface is made with.
    look: Look,
    /// The host's own window.
    main: View,
    /// Checkpoint 6: the windows of the panels that are out.
    panes: [Option<View>; ::folderstorm_session::panels::PANELS],
    /// The window being drawn.
    drawing: Which,
    /// The panels that were out when the host started have their windows.
    restored: bool,
    /// The host's window title; a panel's window adds its name.
    title: String,
    error: Option<String>,
    next_tick: Instant,
    /// The name typed for a new character, and why it was not added.
    new_profile: String,
    add_error: Option<String>,
    /// Checkpoint 4: the hotkeys' and the new profile's pages.
    comfort: comfort::Panel,
    /// The character whose "End process" waits for its confirmation.
    ending: Option<usize>,
    /// The window's close button was pressed while characters run.
    closing: bool,
    /// "Quit all" was pressed and waits for its confirmation (the close
    /// question is one already: its "Quit all" is not asked again).
    quit_asked: bool,
    /// The shared chat's panel.
    chat: ChatPanel,
    /// A narrow window shows the chat (true) or the characters.
    chat_page: bool,
    /// Checkpoint 7: the character whose alias and colour are being
    /// chosen, and what is typed for it.
    identity: identity::Editor,
    /// The count the inbox's own window has in its title.
    inbox_titled: u32,
    /// Checkpoint 8: the bar's compact window, and the host's own window
    /// hidden while a bar is on screen.
    bar: bar::Bar,
    /// Checkpoint 9: each monitor's own window, and the cycling monitor.
    monitors: monitor_windows::Monitors,
    /// Checkpoint 10: the settings' window.
    settings: settings::Settings,
}

impl Window_ {
    fn new(host: Host, look: Look) -> Self {
        let heading = tr!("folderstorm-host-title", product = folderstorm_brand::PRODUCT);
        let title = match &crate::cli::get().title {
            Some(title) => format!("{heading} — {title}"),
            None => heading,
        };
        Self {
            host,
            palette: look.theme.palette(),
            main: look.view(),
            look,
            panes: panels::no_panes(),
            drawing: Which::Main,
            restored: false,
            title,
            error: None,
            next_tick: Instant::now(),
            new_profile: String::new(),
            add_error: None,
            comfort: Default::default(),
            ending: None,
            closing: false,
            quit_asked: false,
            chat: ChatPanel::default(),
            chat_page: false,
            identity: Default::default(),
            inbox_titled: 0,
            bar: Default::default(),
            monitors: Default::default(),
            settings: Default::default(),
        }
    }
}

/// From this width the characters and the chat show side by side.
const WIDE: f32 = 940.0;
/// The width of the characters' column beside the chat.
const CHARACTERS_WIDTH: f32 = 500.0;
/// The same in the condensed layout.
const CONDENSED_WIDTH: f32 = 380.0;
/// The smallest inside of the host's own window, in the screen's points:
/// as it always was, and in the condensed layout.
const MIN_SIZE: [f32; 2] = [440.0, 360.0];
const CONDENSED_MIN_SIZE: [f32; 2] = [340.0, 200.0];

/// The smallest the host's own window gets.
fn min_size(condensed: bool) -> winit::dpi::LogicalSize<f32> {
    let [width, height] = if condensed { CONDENSED_MIN_SIZE } else { MIN_SIZE };
    winit::dpi::LogicalSize::new(width, height)
}

/// What a click in the window asks for; carried out after the frame.
enum Act {
    Switch(usize),
    Close(usize),
    AskEnd(usize),
    End(usize),
    Remove(usize),
    Standby(usize, Standby),
    Stage(bool),
    ChatSound(bool),
    Add,
    /// "Quit all" pressed: asked first.
    AskQuitAll,
    /// Confirmed: every character is closed.
    QuitAll,
    CancelQuit,
    CloseHost,
    Dismiss,
    /// Nothing but the host's look at what changed, at once.
    Refresh,
    /// Checkpoint 4 (`ui/comfort.rs`).
    Comfort(comfort::Act),
    /// Checkpoint 5: a character's monitor opened, changed or closed.
    Monitor(usize, Option<::folderstorm_session::monitor::Spec>),
    /// Checkpoint 6: a panel leaves for its own window, or comes back.
    Panel(Panel, bool),
    /// Checkpoint 7 (`ui/identity.rs`): a character's colour, its alias
    /// (chosen in the settings' window since checkpoint 10).
    Colour(usize, ::folderstorm_session::identity::Colour),
    Alias(usize, String),
    /// Checkpoint 7 (`ui/condensed.rs`): one line per character, or the
    /// cards.
    Condensed(bool),
    /// Follow-ups to checkpoint 7 (`ui/inbox.rs`): a question dismissed
    /// from the host's inbox (character, its viewer's number), all of a
    /// character's, all. The viewers are told nothing.
    InboxDismiss(usize, u64),
    InboxDismissSlot(usize),
    InboxDismissAll,
    /// Checkpoint 8 (`ui/bar.rs`): a click on the bar, from the host's
    /// compact window or its page; a change of the bar's settings.
    Bar(::folderstorm_session::bar::Request),
    BarEdit(bar::Edit),
    /// Checkpoint 9 (`ui/monitor_windows.rs`): a click in a monitor's
    /// window, or on the monitors' page.
    Monitors(monitor_windows::Act),
    /// Checkpoint 10 (`ui/settings.rs`): a click in the settings' window,
    /// or a way to it.
    Settings(settings::Act),
}

impl Window_ {
    /// What the window being drawn (`self.drawing`) shows.
    /// Checkpoint 11: each conversation a viewer opened at the host's
    /// request is shown in the window that asked for it.
    fn show_opened(&mut self) {
        for opened in self.host.book.take_opened() {
            if opened.window == ::folderstorm_session::chat::SHARED_WINDOW {
                self.chat.opened(opened.slot, opened.conversation);
            } else {
                self.monitors.opened(opened.window, opened.slot, opened.conversation);
            }
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) -> Vec<Act> {
        let p = self.palette;
        let now = Instant::now();
        let mut acts = Vec::new();
        self.monitors_begin();
        self.show_opened();
        // (checkpoint 7: the window with the condensed characters keeps
        // less room around what it shows)
        let tight = self.host.desk.condensed() && self.home(Panel::Controls) == self.drawing;
        let frame = egui::Frame::new().fill(p.panel).inner_margin(if tight { 8.0 } else { 18.0 });
        let chat_shown = match self.drawing {
            Which::Main => self.main_content(ui, frame, now, &mut acts),
            Which::Pane(panel) => {
                // a panel's own window: the way back, then the panel
                egui::Panel::top("folderstorm-host-pane-head")
                    .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(18, 6)))
                    .resizable(false)
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.dock_button(ui, &p, panel, &mut acts);
                        });
                    });
                egui::CentralPanel::default().frame(frame).show(ui, |ui| match panel {
                    Panel::Chat => self.chat_part(ui, now, &mut acts),
                    Panel::Controls => self.characters(ui, now, &mut acts),
                    Panel::Inbox => self.inbox_window(ui, &p, now, &mut acts),
                });
                panel == Panel::Chat
            }
            Which::Bar => {
                // the compact window: the row, and nothing else
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(p.panel))
                    .show(ui, |ui| self.bar_window(ui, &mut acts));
                false
            }
            Which::Monitor(_) | Which::Cycling => {
                // a monitor's window: its header, its picture, its chat
                if let Some(seat) = Seat::of(self.drawing) {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(p.panel))
                        .show(ui, |ui| self.monitor_window(ui, seat, now, &mut acts));
                }
                false
            }
            Which::Settings => {
                // (checkpoint 10) the sections' list, then a page
                self.settings_window(ui, &mut acts);
                false
            }
        };
        if self.home(Panel::Chat) == self.drawing {
            // what the chat's window shows is read while that window has
            // the keyboard; the host's other windows say nothing of it
            let viewing = if chat_shown {
                self.chat.viewing(&self.host.chat)
            } else {
                ::folderstorm_session::chat::Viewing::Nothing
            };
            let focused = self.view(self.drawing).is_some_and(|view| view.focused);
            self.host.chat.look(viewing, focused);
        }
        acts
    }

    /// The host's own window: which panels have a window of their own,
    /// then those that have not. True when the chat is on screen in it.
    fn main_content(&mut self, ui: &mut egui::Ui, frame: egui::Frame, now: Instant, acts: &mut Vec<Act>) -> bool {
        let p = self.palette;
        let side = if self.host.desk.condensed() && !self.out(Panel::Controls) {
            8
        } else {
            18
        };
        egui::Panel::top("folderstorm-host-panels")
            .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(side, 6)))
            .resizable(false)
            .show(ui, |ui| self.panel_strip(ui, &p, acts));
        // the inbox, docked: a band over the rest while something waits,
        // whatever the page and whichever panels are out
        let inbox_here = !self.out(Panel::Inbox) && !self.host.inbox.is_empty();
        if inbox_here {
            egui::Panel::top("folderstorm-host-inbox")
                .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(side, 6)))
                .resizable(false)
                .show(ui, |ui| self.inbox_band(ui, &p, now, acts));
        }
        if std::mem::replace(&mut self.chat.inbox_beside, inbox_here) != inbox_here {
            // (the band came or went: the window settles in one more frame)
            ui.ctx().request_repaint();
        }
        let (characters, chat) = (!self.out(Panel::Controls), !self.out(Panel::Chat));
        if !characters && (self.closing || self.host.desk.quitting()) {
            // the characters are in their own window: what closing the
            // host asks is asked here, in the window that was closed
            egui::Panel::bottom("folderstorm-host-closing")
                .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(18, 10)))
                .resizable(false)
                .show(ui, |ui| self.footer(ui, &p, now, acts));
        }
        let wide = characters && chat && ui.available_width() >= WIDE;
        if wide {
            egui::Panel::left("folderstorm-host-characters")
                .frame(frame)
                .exact_size(if self.host.desk.condensed() {
                    CONDENSED_WIDTH
                } else {
                    CHARACTERS_WIDTH
                })
                .resizable(false)
                .show(ui, |ui| self.characters(ui, now, acts));
        }
        let chat_shown = chat && (wide || !characters || self.chat_page);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if !characters && !chat {
                self.panels_elsewhere(ui, &p, acts);
                return;
            }
            if characters && chat && !wide {
                // one page at a time: the characters, or their chat
                let unread = self.host.chat.total_unread();
                let chat = match unread {
                    0 => tr!("folderstorm-chat-heading").to_owned(),
                    n => format!("{} ({})", tr!("folderstorm-chat-heading"), super::chat_ui::count_text(n)),
                };
                let mut page = usize::from(self.chat_page);
                let pages = [(tr!("folderstorm-host-heading"), true), (chat.as_str(), true)];
                if widgets::segmented(ui, &p, &mut page, &pages) {
                    self.chat_page = page == 1;
                }
                ui.add_space(8.0);
            }
            if chat_shown {
                self.chat_part(ui, now, acts);
            } else {
                self.characters(ui, now, acts);
            }
        });
        chat_shown
    }

    /// The shared chat, and what a click in it asks for.
    fn chat_part(&mut self, ui: &mut egui::Ui, now: Instant, acts: &mut Vec<Act>) {
        let p = self.palette;
        let asked = self.chat.show(ui, &p, &mut self.host, now);
        if let Some(slot) = asked.switch {
            acts.push(Act::Switch(slot));
        } else if asked.acted {
            acts.push(Act::Refresh);
        }
    }

    /// The characters' rows, the choices and the footer.
    fn characters(&mut self, ui: &mut egui::Ui, now: Instant, acts: &mut Vec<Act>) {
        let p = self.palette;
        let radius = ui.visuals().window_corner_radius;
        if self.comfort_page(ui, acts) {
            return;
        }
        let quitting = self.host.desk.quitting();
        if self.host.desk.condensed() {
            // one line per character: `ui/condensed.rs`
            self.condensed(ui, &p, now, quitting, acts);
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("folderstorm-host-heading")).size(18.0).strong().color(p.ink));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.condensed_button(ui, &p, acts);
            });
        });
        ui.add_space(2.0);
        ui.label(RichText::new(tr!("folderstorm-host-intro")).color(p.muted));
        ui.add_space(10.0);
        // (checkpoint 10: the stage's and the alert's boxes are in the
        // settings' window)
        self.quick_strip(ui, &p, acts);
        egui::ScrollArea::vertical()
            .id_salt("folderstorm-host-characters")
            .auto_shrink([false, true])
            .max_height((ui.available_height() - 150.0).max(120.0))
            .show(ui, |ui| {
                let slots: Vec<(usize, Slot)> = self.host.desk.slots().map(|(i, s)| (i, s.clone())).collect();
                if slots.is_empty() {
                    ui.label(RichText::new(tr!("folderstorm-host-empty")).color(p.muted));
                }
                for (slot, state) in &slots {
                    // (a scope of its own per card: two cards' choices, the
                    // monitors' sizes and rates, are not one widget)
                    ui.push_id(("folderstorm-host-card", *slot), |ui| {
                        egui::Frame::new()
                            .fill(p.field)
                            .inner_margin(12.0)
                            .corner_radius(radius)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.character(ui, &p, *slot, state, now, quitting, acts);
                                self.monitor_panel(ui, &p, *slot, state, acts);
                            });
                    });
                    ui.add_space(8.0);
                }
            });
        ui.add_space(6.0);
        if let Some(waits) = self.host.desk.voice_waits_for() {
            let name = self.name(waits);
            ui.label(RichText::new(tr!("folderstorm-host-voice-waits", name = name)).color(p.amber));
            ui.add_space(4.0);
        }
        if !quitting && !self.host.desk.full() {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 190.0).max(120.0);
                let field = widgets::text_field(ui, true, &mut self.new_profile, |b, _| {
                    egui::TextEdit::singleline(b)
                        .char_limit(profile::MAX_LEN)
                        .hint_text(tr!("folderstorm-host-add-hint"))
                        .desired_width(width)
                });
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = tr!("folderstorm-host-add");
                if widgets::action_button(ui, &p, "plus", add, ActionKind::Flat, BUTTON).clicked() || entered {
                    acts.push(Act::Add);
                }
            });
            if let Some(error) = &self.add_error {
                ui.label(RichText::new(error).color(p.danger));
            }
        }
        if !quitting {
            // (checkpoint 10: "Settings…" is there with five characters too)
            self.comfort_buttons(ui, &p, acts);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            self.footer(ui, &p, now, acts);
        });
    }

    /// The name a character is called by: its avatar's once logged in, its
    /// profile's before.
    fn name(&self, slot: usize) -> String {
        self.host.desk.slot(slot).map(shown_name).unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    fn character(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, now: Instant, quitting: bool, acts: &mut Vec<Act>) {
        let active = self.host.desk.active() == Some(slot);
        let face = super::faces::face(p, state);
        ui.horizontal_wrapped(|ui| {
            if face.tint.is_some() {
                super::faces::dot(ui, p, face.tint);
            }
            ui.label(RichText::new(&face.name).size(14.5).strong().color(p.ink));
            if let Some(real) = &face.real {
                // behind an alias, the real name stays in sight on the card
                ui.label(RichText::new(real).color(p.muted));
            }
            if state.status.as_ref().is_some_and(|s| !s.name.is_empty()) {
                ui.label(RichText::new(state.profile.shown()).color(p.muted));
            }
            if active {
                ui.label(RichText::new(tr!("folderstorm-host-active")).strong().color(p.violet_light));
            }
            self.voice_badge(ui, p, slot);
            // (checkpoint 7: its alias and colour)
            self.identity_button(ui, p, slot, acts);
        });
        ui.label(RichText::new(status_now(&self.host.desk, slot, state, now)).color(p.muted));
        for (text, serious) in warnings(&self.host.desk, slot, state, now) {
            ui.label(RichText::new(text).color(if serious { p.danger } else { p.amber }));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if state.live() {
                if !active && !state.closing {
                    let switch = tr!("folderstorm-host-switch");
                    if widgets::action_button(ui, p, "arrows-exchange", switch, ActionKind::Primary, BUTTON).clicked() {
                        acts.push(Act::Switch(slot));
                    }
                } else if active {
                    let show = tr!("folderstorm-host-show");
                    if widgets::action_button(ui, p, "arrows-exchange", show, ActionKind::Flat, BUTTON).clicked() {
                        acts.push(Act::Switch(slot));
                    }
                }
                if !state.closing {
                    let close = tr!("folderstorm-host-close");
                    if widgets::action_button(ui, p, "x", close, ActionKind::Flat, BUTTON)
                        .on_hover_text(tr!("folderstorm-host-close-help"))
                        .clicked()
                    {
                        acts.push(Act::Close(slot));
                    }
                }
                if self.ending == Some(slot) {
                    ui.label(RichText::new(tr!("folderstorm-host-end-question")).color(p.danger));
                    let end = tr!("folderstorm-host-end-yes");
                    if widgets::action_button(ui, p, "player-stop-filled", end, ActionKind::Danger, BUTTON).clicked() {
                        acts.push(Act::End(slot));
                    }
                    if widgets::action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked() {
                        acts.push(Act::Dismiss);
                    }
                } else if state.closing
                    || self.host.desk.health(slot, now) != Health::Healthy
                    || state.note.is_some()
                    || self.host.desk.starting_too_long(slot, now)
                {
                    // offered only when something is wrong: never the usual way out
                    let end = tr!("folderstorm-host-end");
                    if widgets::action_button(ui, p, "player-stop-filled", end, ActionKind::FlatDanger, BUTTON)
                        .on_hover_text(tr!("folderstorm-host-end-help"))
                        .clicked()
                    {
                        acts.push(Act::AskEnd(slot));
                    }
                }
            } else if !quitting && state.phase != Phase::Unmanaged {
                // (a viewer let go runs by itself: closed there, by the user)
                let (icon, label) = if matches!(state.phase, Phase::Ended { .. }) {
                    ("refresh", tr!("folderstorm-host-restart"))
                } else {
                    ("player-play-filled", tr!("folderstorm-host-launch"))
                };
                if widgets::action_button(ui, p, icon, label, ActionKind::Primary, BUTTON).clicked() {
                    acts.push(Act::Switch(slot));
                }
                let remove = tr!("folderstorm-host-remove");
                if widgets::action_button(ui, p, "trash-filled", remove, ActionKind::Flat, BUTTON)
                    .on_hover_text(tr!("folderstorm-host-remove-help"))
                    .clicked()
                {
                    acts.push(Act::Remove(slot));
                }
            }
            let mut economy = state.standby == Standby::Economy;
            if widgets::checkbox(ui, p, &mut economy, tr!("folderstorm-host-economy"))
                .on_hover_text(tr!("folderstorm-host-economy-help"))
                .changed()
            {
                acts.push(Act::Standby(slot, if economy { Standby::Economy } else { Standby::Warm }));
            }
            self.sound_choice(ui, p, slot, state, acts);
            self.monitor_choice(ui, p, slot, state, acts);
        });
    }

    fn footer(&self, ui: &mut egui::Ui, p: &Palette, now: Instant, acts: &mut Vec<Act>) {
        let desk = &self.host.desk;
        let close_question = self.closing && self.drawing == Which::Main;
        if self.quit_asked && desk.live() > 0 && !desk.quitting() && !close_question {
            // (follow-ups to checkpoint 7: "Quit all" is confirmed, with
            // how many characters it closes; the question on a line of
            // its own, the buttons under it)
            ui.vertical(|ui| {
                let question = tr!("folderstorm-host-quit-all-question", count = desk.live());
                ui.label(RichText::new(question).color(p.ink));
                ui.horizontal(|ui| {
                    let quit = tr!("folderstorm-host-quit-all");
                    if widgets::action_button(ui, p, "power", quit, ActionKind::Danger, BUTTON).clicked() {
                        acts.push(Act::QuitAll);
                    }
                    if widgets::action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked() {
                        acts.push(Act::Dismiss);
                    }
                });
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if desk.quitting() {
                let text = if desk.quit_overdue(now) {
                    tr!("folderstorm-host-quit-overdue")
                } else {
                    tr!("folderstorm-host-quit-waiting")
                };
                ui.label(RichText::new(text).color(p.amber));
                if widgets::action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked() {
                    acts.push(Act::CancelQuit);
                }
            } else if close_question {
                // (asked in the host's own window: the one being closed)
                ui.label(RichText::new(tr!("folderstorm-host-close-question")).color(p.ink));
                let quit = tr!("folderstorm-host-quit-all");
                if widgets::action_button(ui, p, "power", quit, ActionKind::Danger, BUTTON).clicked() {
                    acts.push(Act::QuitAll);
                }
                let only = tr!("folderstorm-host-close-host-only");
                if widgets::action_button(ui, p, "door-exit", only, ActionKind::Flat, BUTTON)
                    .on_hover_text(tr!("folderstorm-host-close-host-only-help"))
                    .clicked()
                {
                    acts.push(Act::CloseHost);
                }
                if widgets::action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked() {
                    acts.push(Act::Dismiss);
                }
            } else if desk.live() > 0 {
                let quit = tr!("folderstorm-host-quit-all");
                if widgets::action_button(ui, p, "power", quit, ActionKind::Flat, BUTTON)
                    .on_hover_text(tr!("folderstorm-host-quit-all-help"))
                    .clicked()
                {
                    acts.push(Act::AskQuitAll);
                }
            }
        });
    }

    fn apply(&mut self, acts: Vec<Act>, now: Instant) {
        for act in acts {
            match act {
                Act::Switch(slot) => {
                    if let Some(s) = self.host.desk.slot(slot)
                        && !s.live()
                    {
                        log::info!("session host: starting character {}", slot + 1);
                    }
                    self.host.desk.switch(slot, now);
                }
                Act::Close(slot) => self.host.desk.close(slot),
                Act::AskEnd(slot) => self.ending = Some(slot),
                Act::End(slot) => {
                    self.ending = None;
                    self.host.desk.end(slot);
                }
                Act::Remove(slot) => {
                    if self.host.desk.remove(slot) {
                        // nothing of it is left for the next character of that slot
                        self.host.chat.forget(slot);
                        self.host.book.forget(slot);
                        self.host.inbox.clear(slot);
                        self.identity.close_for(slot);
                    }
                }
                Act::Standby(slot, standby) => self.host.desk.set_standby(slot, standby),
                Act::Stage(stage) => self.host.desk.set_stage(stage),
                Act::ChatSound(on) => self.host.desk.set_chat_sound(on),
                Act::Add => match self.host.desk.add(&self.new_profile) {
                    Ok(_) => {
                        self.new_profile.clear();
                        self.add_error = None;
                    }
                    Err(error) => self.add_error = Some(add_error(&self.new_profile, error)),
                },
                Act::AskQuitAll => {
                    self.closing = false;
                    self.quit_asked = true;
                }
                Act::QuitAll => {
                    self.closing = false;
                    self.quit_asked = false;
                    self.host.desk.quit_all(now);
                }
                Act::CancelQuit => self.host.desk.cancel_quit(),
                Act::CloseHost => {
                    log::info!("session host: closing; the characters stay open by themselves");
                    self.host.exit = true;
                }
                Act::Dismiss => {
                    self.ending = None;
                    self.closing = false;
                    self.quit_asked = false;
                }
                Act::Refresh => {}
                Act::Comfort(act) => self.comfort_apply(act, now),
                Act::Monitor(slot, spec) => self.host.desk.set_monitor(slot, spec),
                // (its window opens or closes before the next wait:
                // `sync_panes`)
                Act::Panel(panel, out) => self.host.desk.set_panel_out(panel, out),
                Act::Colour(slot, colour) => self.host.desk.set_colour(slot, colour),
                Act::Alias(slot, text) => self.identity_alias(slot, &text),
                Act::InboxDismiss(slot, id) => {
                    self.host.inbox.dismiss(slot, id);
                }
                Act::InboxDismissSlot(slot) => {
                    self.host.inbox.dismiss_slot(slot);
                }
                Act::InboxDismissAll => {
                    self.host.inbox.dismiss_all();
                }
                Act::Bar(request) => self.bar_apply(request, now),
                Act::BarEdit(edit) => self.bar_edit(edit),
                Act::Monitors(act) => self.monitors_apply(act, now),
                Act::Settings(act) => self.settings_apply(act),
                Act::Condensed(condensed) => {
                    self.host.desk.set_condensed(condensed);
                    // (the cards' pages have no way back from the lines)
                    self.identity_edit(None);
                    if let Some(window) = &self.main.window {
                        window.set_min_inner_size(Some(min_size(condensed)));
                    }
                }
            }
        }
        self.host.tick(now);
        // (checkpoint 9: a monitor whose character came to the front
        // shows the one that left it, in the same window; its window
        // follows before the next wait, `sync_monitors`, never here: this
        // may be the very frame of that window, its pixels still out)
        // (nothing left to quit: the question goes)
        self.quit_asked &= self.host.desk.live() > 0;
        self.redraw_all();
    }

    /// Draws one of the host's windows, on the processor.
    fn render(&mut self, which: Which, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        // the surface and the painter leave the view for the frame: the
        // pixels are borrowed from the surface while the interface, which
        // borrows `self`, is built
        let Some(view) = self.view_mut(which) else {
            return Ok(());
        };
        let Some(mut surface) = view.surface.take() else {
            anyhow::bail!("missing host surface");
        };
        let mut painter = std::mem::take(&mut view.painter);
        self.drawing = which;
        let result = self.render_on(which, &mut surface, &mut painter, event_loop);
        self.drawing = Which::Main;
        // (a panel docked by this very frame keeps its window until the
        // next wait: its view is still there)
        if let Some(view) = self.view_mut(which) {
            view.surface = Some(surface);
            view.painter = painter;
        }
        result
    }

    fn render_on(
        &mut self,
        which: Which,
        surface: &mut softbuffer::Surface<winit::event_loop::OwnedDisplayHandle, std::sync::Arc<Window>>,
        painter: &mut crate::crash::software::Painter,
        event_loop: &ActiveEventLoop,
    ) -> anyhow::Result<()> {
        let Some(view) = self.view_mut(which) else {
            return Ok(());
        };
        let window = view.window.clone().ok_or_else(|| anyhow::anyhow!("missing host window"))?;
        let size = window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        // the pixels first: egui's texture changes must be painted once
        // they are taken, so nothing may fail between the two
        surface.resize(width, height).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut buffer = surface.buffer_mut().map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some(input) = view.input.as_mut() else {
            anyhow::bail!("missing host input");
        };
        let raw = input.take_egui_input(&window);
        let context = view.context.clone();
        let mut acts = Vec::new();
        let mut output = context.run_ui(raw, |ui| acts = self.content(ui));
        let primitives = context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        painter.paint(
            &mut buffer,
            [size.width as usize, size.height as usize],
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            self.palette.panel,
        );
        // painted: egui checks, in builds with assertions, that the
        // changes were not dropped unapplied
        output.textures_delta.clear();
        if let Some(input) = self.view_mut(which).and_then(|view| view.input.as_mut()) {
            input.handle_platform_output_with_event_loop(&window, event_loop, output.platform_output);
        }
        if self.host.demo
            && let Some(path) = std::env::var_os("AURORA_SESSION_HOST_CAPTURE")
        {
            // (demo only: the window's picture, for the docs and the
            // checks; a panel's window adds its name to the file's)
            let path = capture_path(std::path::Path::new(&path), which);
            let rgba: Vec<_> = buffer
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            if let Err(e) = image::save_buffer(path, &rgba, size.width, size.height, image::ColorType::Rgba8) {
                log::warn!("session host: capture not written ({e})");
            }
        }
        buffer.present().map_err(|e| anyhow::anyhow!("{e}"))?;
        let again = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|o| o.repaint_delay < TICK);
        if let Some(view) = self.view_mut(which) {
            view.redraw = again || !acts.is_empty();
        }
        if which == Which::Bar {
            // the compact window makes room for its menus and hovers
            self.bar_fit(&context, &window);
        }
        if !acts.is_empty() {
            self.apply(acts, Instant::now());
        }
        Ok(())
    }
}

/// Where the demo's capture of a window goes: the file named for the
/// host's own window, `<name>-<panel>.<ext>` for a panel's.
fn capture_path(path: &std::path::Path, which: Which) -> std::path::PathBuf {
    let key = match which {
        Which::Main => return path.to_owned(),
        Which::Pane(panel) => panel.key().to_owned(),
        Which::Bar => "bar".to_owned(),
        Which::Monitor(slot) => Seat::Slot(slot).key(),
        Which::Cycling => Seat::Cycling.key(),
        Which::Settings => "settings".to_owned(),
    };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let mut name = format!("{stem}-{key}");
    if let Some(extension) = path.extension() {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    path.with_file_name(name)
}

/// The name a character is shown by: its alias when it has one, else
/// the avatar's name once logged in, the profile's before.
pub(super) fn shown_name(slot: &Slot) -> String {
    super::faces::shown_name(slot)
}

/// Where a character is, in a line.
fn status_line(slot: &Slot) -> String {
    match slot.phase {
        Phase::Stopped => tr!("folderstorm-host-stopped").to_owned(),
        Phase::Starting => tr!("folderstorm-host-starting").to_owned(),
        Phase::Ended { crashed: false } if slot.ended => tr!("folderstorm-host-ended").to_owned(),
        Phase::Ended { crashed: false } => tr!("folderstorm-host-closed").to_owned(),
        Phase::Ended { crashed: true } => tr!("folderstorm-host-crashed").to_owned(),
        Phase::Unmanaged => tr!("folderstorm-host-unmanaged").to_owned(),
        Phase::Running if slot.closing => tr!("folderstorm-host-closing").to_owned(),
        Phase::Running => match &slot.status {
            None => tr!("folderstorm-host-starting").to_owned(),
            Some(status) => match status.screen {
                Screen::Starting => tr!("folderstorm-host-starting").to_owned(),
                Screen::Login => tr!("folderstorm-host-login").to_owned(),
                Screen::Loading => tr!("folderstorm-host-loading").to_owned(),
                Screen::World => {
                    let mut line = if status.region.is_empty() {
                        tr!("folderstorm-host-in-world").to_owned()
                    } else {
                        tr!("folderstorm-host-in-region", region = status.region.clone())
                    };
                    if status.unread > 0 {
                        line.push_str(" · ");
                        line.push_str(&tr!("folderstorm-host-unread", count = status.unread));
                    }
                    if status.window.minimised {
                        line.push_str(" · ");
                        line.push_str(tr!("folderstorm-host-minimised"));
                    }
                    line
                }
            },
        },
    }
}

/// Where a character is, with how long its viewer has been starting: a
/// start that takes a while counts up instead of looking stuck (the
/// viewer's own splash shows on screen meanwhile).
fn status_now(desk: &::folderstorm_session::host::Desk, slot: usize, state: &Slot, now: Instant) -> String {
    let line = status_line(state);
    match desk.starting_for(slot, now).map(|d| d.as_secs()) {
        Some(seconds) if seconds > 0 => format!("{line} · {}", tr!("folderstorm-host-starting-for", seconds = seconds)),
        _ => line,
    }
}

/// What is wrong with a character, each with whether it is serious.
fn warnings(desk: &::folderstorm_session::host::Desk, slot: usize, state: &Slot, now: Instant) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    match state.note {
        Some(Note::SpawnFailed) => lines.push((tr!("folderstorm-host-note-spawn").to_owned(), true)),
        Some(Note::OpenElsewhere) => lines.push((tr!("folderstorm-host-note-elsewhere").to_owned(), false)),
        Some(Note::Refused(Refusal::Build)) => lines.push((tr!("folderstorm-host-note-build").to_owned(), true)),
        Some(Note::Refused(_)) => lines.push((tr!("folderstorm-host-note-refused").to_owned(), true)),
        Some(Note::NoLink) => lines.push((tr!("folderstorm-host-note-no-link").to_owned(), true)),
        None => {}
    }
    if desk.starting_too_long(slot, now) {
        lines.push((tr!("folderstorm-host-start-slow").to_owned(), false));
    }
    match desk.health(slot, now) {
        Health::Healthy => {}
        Health::Slow => lines.push((tr!("folderstorm-host-slow").to_owned(), false)),
        Health::NotAnswering => lines.push((tr!("folderstorm-host-not-answering").to_owned(), true)),
    }
    match state.status.as_ref().map(|s| s.gpu) {
        Some(Gpu::Busy) => lines.push((tr!("folderstorm-host-gpu-busy").to_owned(), false)),
        Some(Gpu::Stalled) => lines.push((tr!("folderstorm-host-gpu-stalled").to_owned(), false)),
        Some(Gpu::Lost) => lines.push((tr!("folderstorm-host-gpu-lost").to_owned(), false)),
        Some(Gpu::Ok) | None => {}
    }
    if let Some(other) = desk.same_account(slot) {
        lines.push((tr!("folderstorm-host-same-account", number = other + 1), true));
    }
    lines
}

/// Why a character was not added, in the words of `--profile`'s refusal.
fn add_error(name: &str, error: AddError) -> String {
    let why = match error {
        AddError::Duplicate => return tr!("folderstorm-host-add-duplicate").to_owned(),
        AddError::Full => return tr!("folderstorm-host-add-full", max = MAX_SLOTS),
        AddError::Name(NameError::Empty) => return tr!("folderstorm-host-add-empty").to_owned(),
        AddError::Name(NameError::TooLong) => tr!("folderstorm-profile-too-long", max = profile::MAX_LEN),
        AddError::Name(NameError::Character(c)) => tr!("folderstorm-profile-character", character = c.to_string()),
        AddError::Name(NameError::Start) => tr!("folderstorm-profile-start").to_owned(),
        AddError::Name(NameError::Reserved) => tr!("folderstorm-profile-reserved").to_owned(),
    };
    tr!("folderstorm-profile-refused", name = name.trim().to_owned(), why = why)
}

impl ApplicationHandler for Window_ {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main.window.is_some() {
            return;
        }
        let attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(1120.0, 760.0))
            .with_min_inner_size(min_size(self.host.desk.condensed()));
        let made = event_loop
            .create_window(attributes)
            .map_err(anyhow::Error::from)
            .and_then(|window| self.main.attach(event_loop, window));
        match made {
            // the panels that were out when the host closed, after the
            // host's own window and without taking the keyboard from it
            Ok(()) => {
                self.sync_panes(event_loop);
                // (checkpoint 10, demo only) the settings, on a section
                if let Some(section) = self.host.demo_settings.take() {
                    self.open_settings(section, None);
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(which) = self.which(id) else {
            return;
        };
        match which {
            Which::Main if matches!(event, WindowEvent::CloseRequested) => {
                if self.bar_closes_host() {
                    // (checkpoint 8) a bar brings it back: it only hides
                    self.redraw_all();
                } else if self.host.desk.live() == 0 {
                    event_loop.exit();
                } else {
                    // characters run: the user says what becomes of them
                    // (the close question replaces a "Quit all" asked)
                    self.closing = true;
                    self.quit_asked = false;
                    self.redraw_all();
                }
                return;
            }
            // a panel's window closed docks the panel: the host stays
            Which::Pane(panel) if self.pane_event(panel, &event) => return,
            // the compact window closed docks the bar: the host stays
            Which::Bar if self.bar_event(&event) => return,
            // a monitor's window closed closes that monitor: the host stays
            Which::Monitor(_) | Which::Cycling if Seat::of(which).is_some_and(|seat| self.monitor_event(seat, &event)) => return,
            // the settings' window closed: nothing else
            Which::Settings if self.settings_event(&event) => return,
            _ => {}
        }
        if let WindowEvent::Focused(focused) = event
            && self.home(Panel::Chat) == which
            && !focused
        {
            // nothing is read in a window the user has left, drawn again
            // or not (a minimised window is not)
            let viewing = self.chat.viewing(&self.host.chat);
            self.host.chat.look(viewing, false);
        }
        let Some(view) = self.view_mut(which) else {
            return;
        };
        if let WindowEvent::Focused(focused) = event {
            view.focused = focused;
        }
        if let WindowEvent::Occluded(occluded) = event {
            // covered whole: its monitors ask nothing until it shows again
            view.occluded = occluded;
        }
        if let (Some(input), Some(window)) = (&mut view.input, &view.window) {
            let response = input.on_window_event(window, &event);
            view.redraw |= response.repaint;
        }
        if matches!(event, WindowEvent::RedrawRequested)
            && let Err(error) = self.render(which, event_loop)
        {
            match which {
                Which::Main => {
                    self.error = Some(error.to_string());
                    event_loop.exit();
                }
                Which::Pane(panel) => {
                    // a panel's window that cannot be drawn is no reason
                    // to close the host: the panel goes back into it
                    log::warn!("session host: {}'s window is not drawn ({error}): docked", panel.key());
                    self.host.desk.panel_closed(panel);
                }
                Which::Bar => {
                    // nor the bar's: it goes back into the viewers
                    log::warn!("session host: the bar's window is not drawn ({error}): docked");
                    self.host
                        .desk
                        .edit_bar(|bar| bar.placement = ::folderstorm_session::bar::Placement::Docked);
                }
                Which::Monitor(_) | Which::Cycling => {
                    // nor a monitor's: that monitor closes
                    if let Some(seat) = Seat::of(which) {
                        self.monitor_failed(seat, &error);
                    }
                }
                Which::Settings => {
                    // nor the settings': they close
                    log::warn!("session host: the settings' window is not drawn ({error}): closed");
                    self.settings_event(&WindowEvent::CloseRequested);
                }
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.main.detach();
        for pane in self
            .panes
            .iter_mut()
            .flatten()
            .chain(&mut self.bar.view)
            .chain(&mut self.settings.view)
        {
            pane.detach();
        }
        for view in self.monitors.views_mut() {
            view.detach();
        }
    }

    /// A viewer's reader thread has something the window should not leave
    /// waiting (a click on that viewer's bar): looked at now.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        self.next_tick = Instant::now();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.host.pressed_hotkeys(now) {
            // a global hotkey: carried out at once, whatever has the keyboard
            self.host.tick(now);
            self.redraw_all();
        }
        if now >= self.next_tick {
            self.monitors_watch(now);
            self.host.tick(now);
            // (checkpoint 8) what the viewers' bars asked for since
            if self.bar_pump(now) {
                self.host.tick(now);
            }
            let tick = if self.host.desk.live() > 0 { TICK } else { IDLE_TICK };
            self.next_tick = now + tick;
            self.redraw_all();
        }
        if std::mem::take(&mut self.host.alert) {
            // a message for a character that is not the active one
            if self.host.desk.chat_sound() {
                super::message_sound();
            }
            // (the taskbar's flash, in the chat's window: never the
            // foreground or the keyboard)
            if let Some(view) = self.view(self.home(Panel::Chat)).filter(|view| !view.focused)
                && let Some(window) = &view.window
            {
                window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }
        if self.host.exit {
            event_loop.exit();
            return;
        }
        // a panel popped out or docked back gets or loses its window
        self.sync_panes(event_loop);
        // (checkpoint 9) and so does each monitor that opened or closed
        self.sync_monitors(event_loop);
        // (checkpoint 10) and the settings' window
        self.sync_settings(event_loop);
        // (checkpoint 8) and the bar's compact window; the host's own
        // window hides or shows; what a bar asked for comes forward
        self.sync_bar(event_loop);
        self.inbox_retitle();
        for view in std::iter::once(&mut self.main)
            .chain(self.panes.iter_mut().flatten())
            .chain(&mut self.bar.view)
            .chain(self.monitors.views_mut())
            .chain(&mut self.settings.view)
        {
            if std::mem::take(&mut view.redraw)
                && let Some(window) = &view.window
            {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

#[cfg(test)]
mod tests {
    use super::super::chat_ui::tests::{heard, host};
    use super::*;
    use ::folderstorm_session::ipc::{Conversation, LineKind};

    /// The host's own window drawn as `render_on` draws it, without a
    /// window: the interface, then the processor's painter into plain
    /// pixels. `FOLDERSTORM_TEST_CAPTURE=<file.png>` keeps the picture.
    pub(super) fn draw(app: &mut Window_, width: u32, height: u32) -> (Vec<String>, Vec<u32>) {
        draw_in(app, Which::Main, width, height)
    }

    /// The same for any of the host's windows.
    pub(super) fn draw_in(app: &mut Window_, which: Which, width: u32, height: u32) -> (Vec<String>, Vec<u32>) {
        draw_with(app, which, width, height, Vec::new())
    }

    /// The same, with what the user does in that frame (a click, a key, a
    /// text). What the frame asks of the window (its `Act`s) is not carried
    /// out: the tests do that themselves.
    pub(super) fn draw_with(app: &mut Window_, which: Which, width: u32, height: u32, events: Vec<egui::Event>) -> (Vec<String>, Vec<u32>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width as f32, height as f32))),
            events,
            focused: true,
            ..Default::default()
        };
        let view = app.view_mut(which).expect("the window's view");
        let context = view.context.clone();
        let mut painter = std::mem::take(&mut view.painter);
        app.drawing = which;
        let mut output = context.run_ui(raw, |ui| {
            app.content(ui);
        });
        app.drawing = Which::Main;
        fn texts(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, into)),
                _ => {}
            }
        }
        let mut drawn = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut drawn);
        }
        let primitives = context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let mut pixels = vec![0u32; (width * height) as usize];
        painter.paint(
            &mut pixels,
            [width as usize, height as usize],
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            app.palette.panel,
        );
        output.textures_delta.clear();
        if let Some(view) = app.view_mut(which) {
            view.painter = painter;
        }
        if let Some(path) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
        (drawn, pixels)
    }

    /// The host's windows without a window: the host's own has the
    /// keyboard.
    pub(super) fn window(host: Host) -> Window_ {
        let look = Look {
            theme: crate::ui::skin::recovery_theme(),
            font_scale: 1.0,
            font: None,
        };
        let mut app = Window_::new(host, look);
        app.main.focused = true;
        app
    }

    #[test]
    fn a_panel_s_capture_has_its_own_file() {
        let path = std::path::Path::new("shots/host.png");
        assert_eq!(capture_path(path, Which::Main), path);
        assert_eq!(
            capture_path(path, Which::Pane(Panel::Chat)),
            std::path::Path::new("shots/host-chat.png")
        );
        assert_eq!(
            capture_path(std::path::Path::new("host"), Which::Pane(Panel::Inbox)),
            std::path::Path::new("host-inbox")
        );
        // each monitor's window, and the cycling monitor's
        assert_eq!(
            capture_path(path, Which::Monitor(1)),
            std::path::Path::new("shots/host-monitor-2.png")
        );
        assert_eq!(capture_path(path, Which::Cycling), std::path::Path::new("shots/host-cycling.png"));
    }

    #[test]
    fn the_window_draws_the_characters_and_their_chat_on_the_processor() {
        let mut host = host();
        heard(
            &mut host,
            0,
            Conversation::Nearby,
            1,
            LineKind::Said,
            "Jane Doe",
            "hello neighbours",
        );
        heard(&mut host, 0, Conversation::Nearby, 2, LineKind::Own, "Sam Resident", "hello Jane");
        heard(
            &mut host,
            1,
            Conversation::Session(0, 0x4a),
            1,
            LineKind::Said,
            "Jane Doe",
            "a private word",
        );
        heard(
            &mut host,
            1,
            Conversation::Session(0, 0x4a),
            2,
            LineKind::Card,
            "Jane Doe",
            "offers a teleport",
        );
        let mut app = window(host);
        // wide: both side by side
        let shows = |drawn: &[String], text: &str| drawn.iter().any(|t| t.contains(text));
        draw(&mut app, 1120, 760);
        let (drawn, pixels) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "Characters") && shows(&drawn, "Chat"), "{drawn:?}");
        assert!(shows(&drawn, "demo-1") && shows(&drawn, "hello neighbours"));
        assert!(shows(&drawn, "[demo-2 › Jane Doe]"));
        let background = pixels[0];
        assert!(
            pixels.iter().filter(|p| **p != background).count() > 10_000,
            "something was painted"
        );
        // the lines on screen in a window that has the keyboard are read
        assert_eq!(app.host.chat.total_unread(), 0);
        // narrow: one page at a time, the characters first
        let (drawn, _) = draw(&mut app, 460, 760);
        assert!(shows(&drawn, "demo-1"));
        assert!(!shows(&drawn, "hello neighbours"), "{drawn:?}");
        app.chat_page = true;
        let (drawn, _) = draw(&mut app, 460, 760);
        assert!(shows(&drawn, "hello neighbours"));
        assert!(!shows(&drawn, "Each character runs in its own viewer"));
        // (wide again, for the picture a capture keeps)
        draw(&mut app, 1120, 760);
    }

    /// "Quit all" asks first, naming how many characters it closes; the
    /// close question is a question already and does not ask again.
    #[test]
    fn quit_all_is_confirmed_once() {
        let now = Instant::now();
        let mut app = window(host());
        for slot in 0..2 {
            app.host.desk.launch(slot, slot == 0);
            app.host.desk.connected(slot, 1, now);
        }
        app.host.desk.take_effects();
        let shows = |drawn: &[String], text: &str| drawn.iter().any(|t| t.contains(text));
        let question = tr!("folderstorm-host-quit-all-question", count = 2usize);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-host-quit-all")) && !shows(&drawn, &question));
        // pressed: asked, nothing closed yet
        app.apply(vec![Act::AskQuitAll], now);
        assert!(!app.host.desk.quitting());
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, &question), "{drawn:?}");
        assert!(shows(&drawn, tr!("common-cancel")));
        // cancelled: nothing happened
        app.apply(vec![Act::Dismiss], now);
        assert!(!app.host.desk.quitting() && !app.quit_asked);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(!shows(&drawn, &question));
        // the same in the condensed layout, then confirmed
        app.host.desk.set_condensed(true);
        app.apply(vec![Act::AskQuitAll], now);
        let (drawn, _) = draw(&mut app, 420, 300);
        assert!(shows(&drawn, &question), "{drawn:?}");
        app.apply(vec![Act::QuitAll], now);
        assert!(app.host.desk.quitting() && !app.quit_asked);
        app.apply(vec![Act::CancelQuit], now);
        app.host.desk.set_condensed(false);
        // the close question's "Quit all" is the answer: no second question
        app.closing = true;
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, tr!("folderstorm-host-close-question")), "{drawn:?}");
        assert!(!shows(&drawn, &question));
        app.apply(vec![Act::QuitAll], now);
        assert!(app.host.desk.quitting() && !app.closing);
    }

    /// Every icon the host's window names is one the viewer embeds: an
    /// unknown name draws a button without its icon.
    #[test]
    fn the_window_s_icons_exist() {
        let source = include_str!("ui.rs");
        let mut names: Vec<&str> = source
            .split("p, \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .filter(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
            .collect();
        names.extend(["refresh", "player-play-filled"]);
        assert!(names.contains(&"arrows-exchange"), "{names:?}");
        for name in names {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
