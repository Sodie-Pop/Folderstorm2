//! The session host's window: one row per character with what its viewer
//! reports and what can be done with it, the stage choice, and "Quit all";
//! beside it (or behind a tab when the window is narrow) the shared chat
//! (`chat_ui.rs`).
//! Drawn on the processor like the crash window (`crash/window.rs`, whose
//! event loop this follows), with the viewer's theme, fonts, icons and
//! catalogues. Checkpoint 4's part (quick actions, hotkeys, "New character
//! profile…") is in `ui/comfort.rs`.

mod comfort;

use super::Host;
use super::chat_ui::ChatPanel;
use crate::theme::Palette;
use crate::ui::{
    fonts,
    icons::Icons,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::host::{AddError, Health, Note, Phase, Slot};
use ::folderstorm_session::ipc::{Gpu, MAX_SLOTS, Refusal, Screen, Standby};
use ::folderstorm_session::profile::{self, NameError};
use aurora_i18n::tr;
use egui::{RichText, Vec2};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
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
    let context = egui::Context::default();
    let settings = crate::settings::Settings::load();
    let theme = crate::ui::skin::recovery_theme();
    theme.apply(&context, settings.font_scale);
    // (drawn on events and ticks: the system font scan is waited for here)
    fonts::fallback::wait();
    fonts::apply(&context, &settings.font);
    let icons = Icons::load(&context, None);
    let mut app = Window_ {
        host,
        palette: theme.palette(),
        context,
        _icons: icons,
        painter: Default::default(),
        surface: None,
        window: None,
        input: None,
        error: None,
        next_tick: Instant::now(),
        redraw: true,
        new_profile: String::new(),
        add_error: None,
        comfort: Default::default(),
        ending: None,
        closing: false,
        chat: ChatPanel::default(),
        chat_page: false,
        focused: false,
    };
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

/// The host and its window.
struct Window_ {
    host: Host,
    palette: Palette,
    context: egui::Context,
    /// (keeps the icons' textures alive)
    _icons: Icons,
    painter: crate::crash::software::Painter,
    surface: Option<softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>>,
    window: Option<Arc<Window>>,
    input: Option<egui_winit::State>,
    error: Option<String>,
    next_tick: Instant,
    redraw: bool,
    /// The name typed for a new character, and why it was not added.
    new_profile: String,
    add_error: Option<String>,
    /// Checkpoint 4: the hotkeys' and the new profile's pages.
    comfort: comfort::Panel,
    /// The character whose "End process" waits for its confirmation.
    ending: Option<usize>,
    /// The window's close button was pressed while characters run.
    closing: bool,
    /// The shared chat's panel.
    chat: ChatPanel,
    /// A narrow window shows the chat (true) or the characters.
    chat_page: bool,
    /// The window has the keyboard: what it shows of the chat is read.
    focused: bool,
}

/// From this width the characters and the chat show side by side.
const WIDE: f32 = 940.0;
/// The width of the characters' column beside the chat.
const CHARACTERS_WIDTH: f32 = 500.0;

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
    QuitAll,
    CancelQuit,
    CloseHost,
    Dismiss,
    /// Nothing but the host's look at what changed, at once.
    Refresh,
    /// Checkpoint 4 (`ui/comfort.rs`).
    Comfort(comfort::Act),
}

impl Window_ {
    fn content(&mut self, ui: &mut egui::Ui) -> Vec<Act> {
        let p = self.palette;
        let now = Instant::now();
        let mut acts = Vec::new();
        let wide = ui.available_width() >= WIDE;
        let frame = egui::Frame::new().fill(p.panel).inner_margin(18.0);
        if wide {
            egui::Panel::left("folderstorm-host-characters")
                .frame(frame)
                .exact_size(CHARACTERS_WIDTH)
                .resizable(false)
                .show(ui, |ui| self.characters(ui, now, &mut acts));
        }
        let chat_shown = wide || self.chat_page;
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if !wide {
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
            if !chat_shown {
                self.characters(ui, now, &mut acts);
                return;
            }
            let asked = self.chat.show(ui, &p, &mut self.host, now);
            if let Some(slot) = asked.switch {
                acts.push(Act::Switch(slot));
            } else if asked.acted {
                acts.push(Act::Refresh);
            }
        });
        let viewing = if chat_shown {
            self.chat.viewing(&self.host.chat)
        } else {
            ::folderstorm_session::chat::Viewing::Nothing
        };
        self.host.chat.look(viewing, self.focused);
        acts
    }

    /// The characters' rows, the choices and the footer.
    fn characters(&mut self, ui: &mut egui::Ui, now: Instant, acts: &mut Vec<Act>) {
        let p = self.palette;
        let radius = ui.visuals().window_corner_radius;
        if self.comfort_page(ui, acts) {
            return;
        }
        // (two blocks deeper than needed: the rows keep the indentation
        // they have in checkpoint 2, whose review fixes merge here)
        {
            {
                ui.label(RichText::new(tr!("folderstorm-host-heading")).size(18.0).strong().color(p.ink));
                ui.add_space(2.0);
                ui.label(RichText::new(tr!("folderstorm-host-intro")).color(p.muted));
                ui.add_space(10.0);
                let mut stage = self.host.desk.stage();
                if widgets::checkbox(ui, &p, &mut stage, tr!("folderstorm-host-stage"))
                    .on_hover_text(tr!("folderstorm-host-stage-help"))
                    .changed()
                {
                    acts.push(Act::Stage(stage));
                }
                let mut sound = self.host.desk.chat_sound();
                if widgets::checkbox(ui, &p, &mut sound, tr!("folderstorm-chat-sound"))
                    .on_hover_text(tr!("folderstorm-chat-sound-help"))
                    .changed()
                {
                    acts.push(Act::ChatSound(sound));
                }
                ui.add_space(10.0);
                self.quick_strip(ui, &p, acts);
                let quitting = self.host.desk.quitting();
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
                            egui::Frame::new()
                                .fill(p.field)
                                .inner_margin(12.0)
                                .corner_radius(radius)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    self.character(ui, &p, *slot, state, now, quitting, acts);
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
                    self.comfort_buttons(ui, &p, acts);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    self.footer(ui, &p, now, acts);
                });
            }
        }
    }

    /// The name a character is called by: its avatar's once logged in, its
    /// profile's before.
    fn name(&self, slot: usize) -> String {
        self.host.desk.slot(slot).map(shown_name).unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    fn character(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, now: Instant, quitting: bool, acts: &mut Vec<Act>) {
        let active = self.host.desk.active() == Some(slot);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(shown_name(state)).size(14.5).strong().color(p.ink));
            if state.status.as_ref().is_some_and(|s| !s.name.is_empty()) {
                ui.label(RichText::new(state.profile.shown()).color(p.muted));
            }
            if active {
                ui.label(RichText::new(tr!("folderstorm-host-active")).strong().color(p.violet_light));
            }
            self.voice_badge(ui, p, slot);
        });
        ui.label(RichText::new(status_line(state)).color(p.muted));
        for (text, serious) in warnings(&self.host.desk, slot, state, now) {
            ui.label(RichText::new(text).color(if serious { p.danger } else { p.amber }));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if state.live() {
                if !active && !state.closing {
                    let switch = tr!("folderstorm-host-switch");
                    if widgets::action_button(ui, p, "switch-horizontal", switch, ActionKind::Primary, BUTTON).clicked() {
                        acts.push(Act::Switch(slot));
                    }
                } else if active {
                    let show = tr!("folderstorm-host-show");
                    if widgets::action_button(ui, p, "switch-horizontal", show, ActionKind::Flat, BUTTON).clicked() {
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
                } else if state.closing || self.host.desk.health(slot, now) != Health::Healthy || state.note.is_some() {
                    // offered only when something is wrong: never the usual way out
                    let end = tr!("folderstorm-host-end");
                    if widgets::action_button(ui, p, "player-stop-filled", end, ActionKind::FlatDanger, BUTTON)
                        .on_hover_text(tr!("folderstorm-host-end-help"))
                        .clicked()
                    {
                        acts.push(Act::AskEnd(slot));
                    }
                }
            } else if !quitting {
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
        });
    }

    fn footer(&self, ui: &mut egui::Ui, p: &Palette, now: Instant, acts: &mut Vec<Act>) {
        let desk = &self.host.desk;
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
            } else if self.closing {
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
                    acts.push(Act::QuitAll);
                }
            }
        });
    }

    fn apply(&mut self, acts: Vec<Act>, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
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
                Act::QuitAll => {
                    self.closing = false;
                    self.host.desk.quit_all(now);
                }
                Act::CancelQuit => self.host.desk.cancel_quit(),
                Act::CloseHost => {
                    log::info!("session host: closing; the characters stay open by themselves");
                    event_loop.exit();
                }
                Act::Dismiss => {
                    self.ending = None;
                    self.closing = false;
                }
                Act::Refresh => {}
                Act::Comfort(act) => self.comfort_apply(act, now),
            }
        }
        self.host.tick(now);
        self.redraw = true;
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        // the surface leaves `self` for the frame: the pixels are borrowed
        // from it while the interface, which borrows `self`, is built
        let mut surface = self.surface.take().ok_or_else(|| anyhow::anyhow!("missing host surface"))?;
        let result = self.render_on(&mut surface, event_loop);
        self.surface = Some(surface);
        result
    }

    fn render_on(
        &mut self,
        surface: &mut softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
        event_loop: &ActiveEventLoop,
    ) -> anyhow::Result<()> {
        let window = self.window.as_ref().ok_or_else(|| anyhow::anyhow!("missing host window"))?.clone();
        let size = window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        // the pixels first: egui's texture changes must be painted once
        // they are taken, so nothing may fail between the two
        surface.resize(width, height).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut buffer = surface.buffer_mut().map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some(input) = self.input.as_mut() else {
            anyhow::bail!("missing host input");
        };
        let raw = input.take_egui_input(&window);
        let context = self.context.clone();
        let mut acts = Vec::new();
        let mut output = context.run_ui(raw, |ui| acts = self.content(ui));
        let primitives = context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        self.painter.paint(
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
        if let Some(input) = self.input.as_mut() {
            input.handle_platform_output_with_event_loop(&window, event_loop, output.platform_output);
        }
        if self.host.demo
            && let Some(path) = std::env::var_os("AURORA_SESSION_HOST_CAPTURE")
        {
            // (demo only: the window's picture, for the docs and the checks)
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
        self.redraw = again || !acts.is_empty();
        if !acts.is_empty() {
            self.apply(acts, event_loop);
        }
        Ok(())
    }
}

/// The avatar's name once logged in, the profile's before.
pub(super) fn shown_name(slot: &Slot) -> String {
    match slot.status.as_ref().filter(|s| !s.name.is_empty()) {
        Some(status) => status.name.clone(),
        None => slot.profile.shown().to_owned(),
    }
}

/// Where a character is, in a line.
fn status_line(slot: &Slot) -> String {
    match slot.phase {
        Phase::Stopped => tr!("folderstorm-host-stopped").to_owned(),
        Phase::Starting => tr!("folderstorm-host-starting").to_owned(),
        Phase::Ended { crashed: false } => tr!("folderstorm-host-closed").to_owned(),
        Phase::Ended { crashed: true } => tr!("folderstorm-host-crashed").to_owned(),
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

/// What is wrong with a character, each with whether it is serious.
fn warnings(desk: &::folderstorm_session::host::Desk, slot: usize, state: &Slot, now: Instant) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    match state.note {
        Some(Note::SpawnFailed) => lines.push((tr!("folderstorm-host-note-spawn").to_owned(), true)),
        Some(Note::OpenElsewhere) => lines.push((tr!("folderstorm-host-note-elsewhere").to_owned(), false)),
        Some(Note::Refused(Refusal::Build)) => lines.push((tr!("folderstorm-host-note-build").to_owned(), true)),
        Some(Note::Refused(_)) => lines.push((tr!("folderstorm-host-note-refused").to_owned(), true)),
        None => {}
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
        if self.window.is_some() {
            return;
        }
        let setup = || -> anyhow::Result<_> {
            let heading = tr!("folderstorm-host-title", product = folderstorm_brand::PRODUCT);
            let title = match &crate::cli::get().title {
                Some(title) => format!("{heading} — {title}"),
                None => heading,
            };
            let window = Arc::new(
                event_loop.create_window(
                    crate::cli::with_desktop_id(Window::default_attributes())
                        .with_title(title)
                        .with_inner_size(winit::dpi::LogicalSize::new(1120.0, 760.0))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(440.0, 360.0)),
                )?,
            );
            let display = softbuffer::Context::new(event_loop.owned_display_handle()).map_err(|e| anyhow::anyhow!("{e}"))?;
            let surface = softbuffer::Surface::new(&display, window.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
            let input = egui_winit::State::new(
                self.context.clone(),
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                None,
                None,
            );
            Ok((window, surface, input))
        };
        match setup() {
            Ok((window, surface, input)) => {
                window.request_redraw();
                self.window = Some(window);
                self.surface = Some(surface);
                self.input = Some(input);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            if self.host.desk.live() == 0 {
                event_loop.exit();
            } else {
                // characters run: the user says what becomes of them
                self.closing = true;
                self.redraw = true;
            }
            return;
        }
        if let WindowEvent::Focused(focused) = event {
            self.focused = focused;
        }
        if let (Some(input), Some(window)) = (&mut self.input, &self.window) {
            let response = input.on_window_event(window, &event);
            self.redraw |= response.repaint;
        }
        if matches!(event, WindowEvent::RedrawRequested)
            && let Err(error) = self.render(event_loop)
        {
            self.error = Some(error.to_string());
            event_loop.exit();
        }
    }

    /// As the crash window does: egui-winit's state and the window go
    /// while the event loop still lives (Wayland).
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.input = None;
        self.surface = None;
        self.window = None;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.host.pressed_hotkeys(now) {
            // a global hotkey: carried out at once, whatever has the keyboard
            self.host.tick(now);
            self.redraw = true;
        }
        if now >= self.next_tick {
            self.host.tick(now);
            let tick = if self.host.desk.live() > 0 { TICK } else { IDLE_TICK };
            self.next_tick = now + tick;
            self.redraw = true;
        }
        if std::mem::take(&mut self.host.alert) {
            // a message for a character that is not the active one
            if self.host.desk.chat_sound() {
                super::message_sound();
            }
            if let Some(window) = self.window.as_ref().filter(|_| !self.focused) {
                window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }
        if self.host.exit {
            event_loop.exit();
            return;
        }
        if std::mem::take(&mut self.redraw)
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

#[cfg(test)]
mod tests {
    use super::super::chat_ui::tests::{heard, host};
    use super::*;
    use ::folderstorm_session::ipc::{Conversation, LineKind};

    /// The whole window drawn as `render_on` draws it, without a window:
    /// the interface, then the processor's painter into plain pixels.
    /// `FOLDERSTORM_TEST_CAPTURE=<file.png>` keeps the picture.
    pub(super) fn draw(app: &mut Window_, width: u32, height: u32) -> (Vec<String>, Vec<u32>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width as f32, height as f32))),
            ..Default::default()
        };
        let context = app.context.clone();
        let mut output = context.run_ui(raw, |ui| {
            app.content(ui);
        });
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
        app.painter.paint(
            &mut pixels,
            [width as usize, height as usize],
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
            app.palette.panel,
        );
        output.textures_delta.clear();
        if let Some(path) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
        (drawn, pixels)
    }

    pub(super) fn window(host: Host) -> Window_ {
        let context = egui::Context::default();
        let theme = crate::ui::skin::recovery_theme();
        theme.apply(&context, 1.0);
        let icons = Icons::load(&context, None);
        Window_ {
            host,
            palette: theme.palette(),
            context,
            _icons: icons,
            painter: Default::default(),
            surface: None,
            window: None,
            input: None,
            error: None,
            next_tick: Instant::now(),
            redraw: true,
            new_profile: String::new(),
            add_error: None,
            comfort: Default::default(),
            ending: None,
            closing: false,
            chat: ChatPanel::default(),
            chat_page: false,
            focused: true,
        }
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
}
