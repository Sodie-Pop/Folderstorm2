//! The window's side of checkpoint 10, the host's settings in one window
//! (session-host plan, section 15): a list of sections on the left, a
//! dense page on the right. The rules of the settings it adds are the
//! `folderstorm-session` crate's (`settings`); the sections that were
//! pages of the characters' panel are drawn by their own files
//! (`ui/bar.rs`, `ui/comfort.rs`, `ui/monitor_windows.rs`), only called
//! from here.
//!
//! - **One window**, opened from the bar's cog menu (each of its entries
//!   on its section) and from the host's window. It does not take the
//!   keyboard by itself unless the user asked for it, and closing it
//!   changes nothing but that it is closed. Its place and the section it
//!   showed are remembered.
//! - **Nothing here is said to a viewer.** Every change goes to the desk,
//!   as the click it replaces did.

use super::panels::{View, screens};
use super::{Act as HostAct, BUTTON, Window_};
use crate::theme::Palette;
use crate::ui::{
    menu,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::ipc::{MAX_SLOTS, Standby};
use ::folderstorm_session::panels::{Place, on_screen};
use ::folderstorm_session::settings::{
    AUTO_READ_MAX_S, AUTO_READ_MIN_S, Alerts, Attention, ChatShown, Reading, Section, WINDOW_MIN_SIZE, WINDOW_SIZE,
};
use aurora_i18n::tr;
use egui::RichText;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// What the window keeps for the settings.
#[derive(Default)]
pub(super) struct Settings {
    /// The settings window, while it is open.
    pub view: Option<View>,
    /// It is wanted open.
    open: bool,
    /// It comes forward at the next wait (asked for by a click).
    raise: bool,
    /// The character the Characters section shows.
    slot: usize,
    /// The Advanced section shows the diagnostics, the icons' credits.
    diagnostics: bool,
    credits: bool,
    /// What "Recover window positions" did, to say it.
    recovered: Option<usize>,
}

/// A click in the settings window, or a way to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// The window opens (in front) on that section, with that character
    /// for the Characters section.
    Open(Section, Option<usize>),
    Section(Section),
    Character(usize),
    ChatShown(ChatShown),
    Reading(Reading),
    Attention(Attention),
    Included(usize, bool),
    Alerts(usize, Alerts),
    /// Two characters' cards swap places.
    Swap(usize, usize),
    /// The host's alert, as it sounds.
    Preview,
    OpenFolder,
    Recover,
    Diagnostics(bool),
    Credits(bool),
}

/// The width of the sections' list, and from which width of the window it
/// shows icons and names (narrower: the names only fit cut).
const LIST_WIDTH: f32 = 196.0;
const NARROW_LIST_WIDTH: f32 = 164.0;
const NARROW: f32 = 640.0;
/// From this width of a page its groups stand in two columns.
const TWO_COLUMNS: f32 = 640.0;
/// The width of a row's name before its choice.
const ROW_NAME: f32 = 170.0;

/// The Tabler Icons' licence, shown in the icons' credits.
const TABLER_LICENCE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/tabler/LICENSE"));

pub(super) fn section_name(section: Section) -> &'static str {
    match section {
        Section::Toolbar => tr!("folderstorm-settings-toolbar"),
        Section::Characters => tr!("folderstorm-settings-characters"),
        Section::Chat => tr!("folderstorm-settings-chat"),
        Section::Sound => tr!("folderstorm-settings-sound"),
        Section::Monitors => tr!("folderstorm-settings-monitors"),
        Section::Hotkeys => tr!("folderstorm-settings-hotkeys"),
        Section::Advanced => tr!("folderstorm-settings-advanced"),
        Section::Updates => tr!("folderstorm-settings-updates"),
    }
}

fn section_intro(section: Section) -> &'static str {
    match section {
        Section::Toolbar => tr!("folderstorm-settings-toolbar-intro"),
        Section::Characters => tr!("folderstorm-settings-characters-intro"),
        Section::Chat => tr!("folderstorm-settings-chat-intro"),
        Section::Sound => tr!("folderstorm-settings-sound-intro"),
        Section::Monitors => tr!("folderstorm-monitors-page-intro"),
        Section::Hotkeys => tr!("folderstorm-hotkeys-intro"),
        Section::Advanced => tr!("folderstorm-settings-advanced-intro"),
        Section::Updates => tr!("folderstorm-settings-updates-intro"),
    }
}

pub(super) fn section_icon(section: Section) -> &'static str {
    match section {
        Section::Toolbar => "app-window-filled",
        Section::Characters => "users",
        Section::Chat => "messages-filled",
        Section::Sound => "volume",
        Section::Monitors => "device-desktop-filled",
        Section::Hotkeys => "keyboard-filled",
        Section::Advanced => "adjustments-filled",
        Section::Updates => "download-filled",
    }
}

/// A group's heading inside a page.
fn heading(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).strong().color(p.ink));
    ui.add_space(2.0);
}

fn note(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(p.muted));
}

/// A row's name in a column of [`ROW_NAME`], then its choice.
fn row(ui: &mut egui::Ui, p: &Palette, name: &str, choice: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let width = ROW_NAME.min(ui.available_width() * 0.4);
        ui.allocate_ui_with_layout(egui::vec2(width, BUTTON.y), layout, |ui| {
            ui.set_min_width(width);
            ui.add(egui::Label::new(RichText::new(name).color(p.muted)).truncate());
        });
        choice(ui);
    });
}

/// Two groups side by side on a wide page, one under the other on a
/// narrow one; both are handed `cx`, one after the other.
fn two<T>(ui: &mut egui::Ui, cx: &mut T, left: impl FnOnce(&mut egui::Ui, &mut T), right: impl FnOnce(&mut egui::Ui, &mut T)) {
    if ui.available_width() >= TWO_COLUMNS {
        ui.columns(2, |columns| {
            left(&mut columns[0], cx);
            right(&mut columns[1], cx);
        });
    } else {
        left(ui, cx);
        ui.add_space(8.0);
        right(ui, cx);
    }
}

impl Window_ {
    /// The settings window opens on that section (and character), in
    /// front.
    pub(super) fn open_settings(&mut self, section: Section, slot: Option<usize>) {
        self.settings.open = true;
        self.settings.raise = true;
        if let Some(slot) = slot {
            self.settings_character(slot);
        }
        self.host.desk.edit_settings_window(|window| window.section = section);
        self.redraw_all();
    }

    /// The settings window is wanted open.
    pub(super) fn settings_wanted(&self) -> bool {
        self.settings.open
    }

    /// The Characters section shows that character.
    fn settings_character(&mut self, slot: usize) {
        if self.host.desk.slot(slot).is_some() {
            self.settings.slot = slot;
        }
    }

    /// The character the Characters section shows: the one chosen, or
    /// the active one, or the first.
    fn settings_slot(&self) -> Option<usize> {
        let desk = &self.host.desk;
        if desk.slot(self.settings.slot).is_some() {
            return Some(self.settings.slot);
        }
        desk.active().or_else(|| desk.slots().map(|(slot, _)| slot).next())
    }

    pub(super) fn settings_apply(&mut self, act: Act) {
        match act {
            Act::Open(section, slot) => self.open_settings(section, slot),
            Act::Section(section) => {
                self.host.desk.edit_settings_window(|window| window.section = section);
            }
            Act::Character(slot) => self.settings_character(slot),
            Act::ChatShown(shown) => self.host.desk.set_chat_shown(shown),
            Act::Reading(reading) => self.host.desk.set_reading(reading),
            Act::Attention(attention) => self.host.desk.set_attention(attention),
            Act::Included(slot, included) => self.host.desk.set_attention_included(slot, included),
            Act::Alerts(slot, alerts) => self.host.desk.set_alerts(slot, alerts),
            Act::Swap(slot, other) => {
                if self.host.desk.swap_slots(slot, other) {
                    // nothing of either stays with the other's number
                    for slot in [slot, other] {
                        self.host.chat.forget(slot);
                        self.host.inbox.clear(slot);
                    }
                    self.identity.close_for(slot);
                    self.identity.close_for(other);
                    self.settings.slot = other;
                }
            }
            Act::Preview => super::super::message_sound(),
            Act::OpenFolder => {
                let folder = crate::settings::config_dir();
                if let Err(e) = crate::open_path::open(&folder) {
                    log::warn!("session host: the settings folder was not opened ({e})");
                }
            }
            Act::Recover => {
                let count = self.recover_windows();
                log::info!("session host: {count} window(s) brought back onto a screen");
                self.settings.recovered = Some(count);
            }
            Act::Diagnostics(on) => self.settings.diagnostics = on,
            Act::Credits(on) => self.settings.credits = on,
        }
    }

    // ---- the window ----

    fn settings_title(&self) -> String {
        format!("{} — {}", tr!("folderstorm-settings-title"), self.title)
    }

    /// The settings window opens or closes as it is wanted, and comes
    /// forward when asked for.
    pub(super) fn sync_settings(&mut self, event_loop: &ActiveEventLoop) {
        match (self.settings.open, self.settings.view.is_some()) {
            (true, false) => match self.open_settings_window(event_loop) {
                Ok(view) => {
                    log::info!("session host: the settings' window opened");
                    self.settings.view = Some(view);
                    self.settings.raise = false;
                    self.redraw_all();
                }
                Err(e) => {
                    log::warn!("session host: no window for the settings ({e})");
                    self.settings.open = false;
                }
            },
            (false, true) => {
                if let Some(mut view) = self.settings.view.take() {
                    view.detach();
                }
            }
            (true, true) if std::mem::take(&mut self.settings.raise) => {
                if let Some(window) = self.settings.view.as_ref().and_then(|view| view.window.as_ref()) {
                    window.set_minimized(false);
                    crate::folderstorm_characters::foreground::bring_forward(window);
                }
            }
            _ => {}
        }
    }

    fn open_settings_window(&self, event_loop: &ActiveEventLoop) -> anyhow::Result<View> {
        use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
        let mut attributes = crate::cli::with_desktop_id(Window::default_attributes())
            .with_title(self.settings_title())
            .with_inner_size(LogicalSize::new(WINDOW_SIZE[0], WINDOW_SIZE[1]))
            .with_min_inner_size(LogicalSize::new(WINDOW_MIN_SIZE[0], WINDOW_MIN_SIZE[1]));
        if let Some(saved) = self.host.desk.settings_window().place {
            let place = on_screen(saved, &screens(event_loop));
            attributes = attributes.with_inner_size(PhysicalSize::new(place.size[0], place.size[1]));
            if let Some([x, y]) = place.position {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
        }
        let mut view = self.look.view();
        view.attach(event_loop, event_loop.create_window(attributes)?)?;
        Ok(view)
    }

    /// What the settings window is told, before the interface hears it.
    /// True when it is gone (closed).
    pub(super) fn settings_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => {
                self.settings.open = false;
                if let Some(mut view) = self.settings.view.take() {
                    view.detach();
                }
                log::info!("session host: the settings' window closed");
                true
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(place) = self.settings.view.as_ref().and_then(window_place) {
                    self.host.desk.edit_settings_window(|window| window.place = Some(place));
                }
                false
            }
            _ => false,
        }
    }

    /// Every window of the host, brought onto a connected screen at the
    /// size it has: its own, the panels', the monitors', the bar's and the
    /// settings'. No character's window is touched. How many moved.
    fn recover_windows(&mut self) -> usize {
        let Some(screens) = self.main.window.as_ref().map(|window| {
            window
                .available_monitors()
                .map(|monitor| ::folderstorm_session::panels::Screen {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                })
                .collect::<Vec<_>>()
        }) else {
            return 0;
        };
        if screens.is_empty() {
            return 0;
        }
        let mut moved = 0;
        let views = std::iter::once(&self.main)
            .chain(self.panes.iter().flatten())
            .chain(&self.bar.view)
            .chain(self.monitors.views().map(|(_, view)| view))
            .chain(&self.settings.view);
        for view in views {
            let Some(window) = &view.window else {
                continue;
            };
            if window.is_minimized().unwrap_or(false) || window.is_maximized() {
                continue;
            }
            let (Ok(at), size) = (window.outer_position(), window.outer_size()) else {
                continue;
            };
            let place = Place {
                position: Some([at.x, at.y]),
                size: [size.width.max(1), size.height.max(1)],
            };
            let back = on_screen(place, &screens);
            if back.position != place.position
                && let Some([x, y]) = back.position
            {
                window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
                moved += 1;
            }
        }
        moved
    }

    // ---- its content ----

    /// The settings window's content: the sections' list, then the page.
    pub(super) fn settings_window(&mut self, ui: &mut egui::Ui, acts: &mut Vec<HostAct>) {
        let p = self.palette;
        let section = self.host.desk.settings_window().section;
        let list_width = if ui.available_width() < NARROW {
            NARROW_LIST_WIDTH
        } else {
            LIST_WIDTH
        };
        egui::Panel::left("folderstorm-settings-sections")
            .frame(egui::Frame::new().fill(p.bar).inner_margin(egui::Margin::symmetric(8, 12)))
            .resizable(false)
            .show_separator_line(false)
            .exact_size(list_width)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for each in Section::ALL {
                    let name = section_name(each);
                    let row = widgets::rail_row(ui, &p, section_icon(each), p.muted, name, None, each == section, 0);
                    if row.on_hover_text(section_intro(each)).clicked() && each != section {
                        acts.push(HostAct::Settings(Act::Section(each)));
                    }
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(20, 14)))
            .show(ui, |ui| {
                ui.label(RichText::new(section_name(section)).size(18.0).strong().color(p.ink));
                ui.add_space(2.0);
                note(ui, &p, section_intro(section));
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt(("folderstorm-settings-page", section.key()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match section {
                            Section::Toolbar => self.toolbar_settings(ui, &p, acts),
                            Section::Characters => self.characters_settings(ui, &p, acts),
                            Section::Chat => self.chat_settings(ui, &p, acts),
                            Section::Sound => self.sound_settings(ui, &p, acts),
                            Section::Monitors => self.monitors_settings(ui, &p, acts),
                            Section::Hotkeys => {
                                let mut asked = Vec::new();
                                self.hotkeys_settings(ui, &p, &mut asked);
                                acts.extend(asked.into_iter().map(HostAct::Comfort));
                            }
                            Section::Advanced => self.advanced_settings(ui, &p, acts),
                            Section::Updates => self.updates_settings(ui, &p),
                        }
                        ui.add_space(12.0);
                    });
            });
    }

    /// One character's own settings, the character chosen at the top.
    fn characters_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let Some(slot) = self.settings_slot() else {
            note(ui, p, tr!("folderstorm-host-empty"));
            return;
        };
        let Some(state) = self.host.desk.slot(slot).cloned() else {
            return;
        };
        if self.identity.slot() != Some(slot) {
            // the alias typed is the chosen character's
            self.identity_edit(Some(slot));
        }
        // which character
        row(ui, p, tr!("folderstorm-settings-character"), |ui| {
            let label = format!("{}: {}", slot + 1, super::shown_name(&state));
            let button = widgets::action_menu_button(ui, p, "users", &label, false, BUTTON.y);
            menu::dropdown(&button, p, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                for (each, other) in self.host.desk.slots() {
                    let label = format!("{}: {}", each + 1, super::shown_name(other));
                    if menu::check(ui, p, "users", &label, each == slot, true) && each != slot {
                        acts.push(HostAct::Settings(Act::Character(each)));
                    }
                }
            });
        });
        ui.add_space(6.0);
        two(
            ui,
            &mut (&mut *self, &mut *acts),
            |ui, (this, acts)| {
                heading(ui, p, tr!("folderstorm-settings-profile"));
                row(ui, p, tr!("folderstorm-settings-profile-name"), |ui| {
                    ui.label(RichText::new(state.profile.shown()).color(p.ink));
                });
                row(ui, p, tr!("folderstorm-settings-logged-in-as"), |ui| {
                    let name = state.status.as_ref().map(|s| s.name.clone()).filter(|n| !n.is_empty());
                    match name {
                        Some(name) => ui.label(RichText::new(name).color(p.ink)),
                        None => ui.label(RichText::new(tr!("folderstorm-settings-not-logged-in")).color(p.muted)),
                    };
                });
                ui.add_space(4.0);
                this.identity_fields(ui, p, slot, &state, acts);
            },
            |ui, (this, acts)| {
                heading(ui, p, tr!("folderstorm-settings-background"));
                let mut mode = usize::from(state.standby == Standby::Economy);
                row(ui, p, tr!("folderstorm-settings-background-mode"), |ui| {
                    let modes = [
                        (tr!("folderstorm-settings-warm"), true),
                        (tr!("folderstorm-settings-economy"), true),
                    ];
                    if widgets::segmented(ui, p, &mut mode, &modes) {
                        let standby = if mode == 1 { Standby::Economy } else { Standby::Warm };
                        acts.push(HostAct::Standby(slot, standby));
                    }
                });
                note(ui, p, tr!("folderstorm-host-economy-help"));
                ui.add_space(4.0);
                this.sound_choice(ui, p, slot, &state, acts);
                let mut included = state.attention;
                if widgets::checkbox(ui, p, &mut included, tr!("folderstorm-settings-include-attention"))
                    .on_hover_text(tr!("folderstorm-settings-include-attention-help"))
                    .changed()
                {
                    acts.push(HostAct::Settings(Act::Included(slot, included)));
                }
                ui.add_space(4.0);
                heading(ui, p, tr!("folderstorm-settings-position"));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr!("folderstorm-settings-position-now", number = slot + 1)).color(p.ink));
                    for (up, icon, label) in [
                        (true, "arrow-up", tr!("folderstorm-settings-move-up")),
                        (false, "arrow-down", tr!("folderstorm-settings-move-down")),
                    ] {
                        let other = this.host.desk.neighbour_card(slot, up);
                        let can = other.is_some_and(|other| this.host.desk.can_swap(slot, other));
                        let button = widgets::action_button_if(ui, p, icon, label, ActionKind::Flat, BUTTON, can);
                        if button.on_hover_text(tr!("folderstorm-settings-position-help")).clicked()
                            && can
                            && let Some(other) = other
                        {
                            acts.push(HostAct::Settings(Act::Swap(slot, other)));
                        }
                    }
                });
            },
        );
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-settings-alerts"));
        note(ui, p, tr!("folderstorm-settings-alerts-intro"));
        let alerts = state.alerts;
        ui.horizontal_wrapped(|ui| {
            let mut edited = alerts;
            for (on, label) in [
                (&mut edited.ims, tr!("folderstorm-settings-ims")),
                (&mut edited.groups, tr!("folderstorm-settings-groups")),
                (&mut edited.conferences, tr!("folderstorm-settings-conferences")),
                (&mut edited.notifications, tr!("folderstorm-settings-notification-alerts")),
            ] {
                widgets::checkbox(ui, p, on, label);
            }
            if edited != alerts {
                acts.push(HostAct::Settings(Act::Alerts(slot, edited)));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if widgets::action_button(
                ui,
                p,
                "player-play-filled",
                tr!("folderstorm-settings-preview"),
                ActionKind::Flat,
                BUTTON,
            )
            .on_hover_text(tr!("folderstorm-settings-preview-help"))
            .clicked()
            {
                acts.push(HostAct::Settings(Act::Preview));
            }
            let reset = tr!("folderstorm-settings-reset");
            if widgets::action_button_if(ui, p, "refresh", reset, ActionKind::Flat, BUTTON, !alerts.is_default()).clicked() {
                acts.push(HostAct::Settings(Act::Alerts(slot, Alerts::default())));
            }
        });
        if !self.host.desk.chat_sound() {
            note(ui, p, tr!("folderstorm-settings-alerts-off"));
        }
    }

    /// What the chats show, what Attention lists, reading.
    fn chat_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let shown = self.host.desk.chat_shown();
        let attention = self.host.desk.attention();
        let reading = self.host.desk.reading();
        two(
            ui,
            acts,
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-chat-shown"));
                note(ui, p, tr!("folderstorm-settings-chat-shown-intro"));
                let mut edited = shown;
                for (on, label) in [
                    (&mut edited.nearby, tr!("folderstorm-settings-nearby")),
                    (&mut edited.ims, tr!("folderstorm-settings-ims")),
                    (&mut edited.groups, tr!("folderstorm-settings-groups")),
                    (&mut edited.conferences, tr!("folderstorm-settings-conferences")),
                    (&mut edited.notices, tr!("folderstorm-settings-notices")),
                ] {
                    widgets::checkbox(ui, p, on, label);
                }
                if edited != shown {
                    acts.push(HostAct::Settings(Act::ChatShown(edited)));
                }
            },
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-attention"));
                note(ui, p, tr!("folderstorm-settings-attention-intro"));
                let mut edited = attention;
                for (on, label) in [
                    (&mut edited.notices, tr!("folderstorm-settings-notices")),
                    (&mut edited.teleports, tr!("folderstorm-settings-teleports")),
                    (&mut edited.permissions, tr!("folderstorm-settings-permissions")),
                    (&mut edited.inventory, tr!("folderstorm-settings-inventory")),
                ] {
                    widgets::checkbox(ui, p, on, label);
                }
                ui.horizontal_wrapped(|ui| {
                    if widgets::radio(ui, p, !edited.newest_first, tr!("folderstorm-settings-oldest-first")).clicked() {
                        edited.newest_first = false;
                    }
                    if widgets::radio(ui, p, edited.newest_first, tr!("folderstorm-settings-newest-first")).clicked() {
                        edited.newest_first = true;
                    }
                });
                if edited != attention {
                    acts.push(HostAct::Settings(Act::Attention(edited)));
                }
                let open = tr!("folderstorm-settings-open-inbox");
                if widgets::action_button(ui, p, "bell-filled", open, ActionKind::Flat, BUTTON).clicked() {
                    acts.push(HostAct::Bar(::folderstorm_session::bar::Request::Inbox));
                }
            },
        );
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-settings-reading"));
        let mut edited = reading;
        widgets::checkbox(ui, p, &mut edited.mark_displayed, tr!("folderstorm-settings-mark-displayed"))
            .on_hover_text(tr!("folderstorm-settings-mark-displayed-help"));
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, p, &mut edited.auto_read, tr!("folderstorm-settings-auto-read"))
                .on_hover_text(tr!("folderstorm-settings-auto-read-help"));
            ui.add_enabled(
                edited.auto_read,
                egui::DragValue::new(&mut edited.auto_read_after_s)
                    .range(AUTO_READ_MIN_S..=AUTO_READ_MAX_S)
                    .suffix(tr!("folderstorm-settings-seconds-suffix")),
            );
        });
        if edited != reading {
            acts.push(HostAct::Settings(Act::Reading(edited)));
        }
    }

    /// Voice, background sound, the host's alert; each character's "heard
    /// in the background" together.
    fn sound_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        use ::folderstorm_session::bar::Request;
        let desk = &self.host.desk;
        two(
            ui,
            acts,
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-voice-and-sound"));
                let mut follows = desk.voice_follows();
                if widgets::checkbox(ui, p, &mut follows, tr!("folderstorm-bar-voice-long"))
                    .on_hover_text(tr!("folderstorm-bar-voice-off-help"))
                    .changed()
                {
                    acts.push(HostAct::Bar(Request::VoiceFollows(follows)));
                }
                let mut mute = desk.mute_background();
                if widgets::checkbox(ui, p, &mut mute, tr!("folderstorm-bar-mute-long"))
                    .on_hover_text(tr!("folderstorm-bar-mute-off-help"))
                    .changed()
                {
                    acts.push(HostAct::Bar(Request::MuteBackground(mute)));
                }
                ui.add_space(6.0);
                heading(ui, p, tr!("folderstorm-settings-alert"));
                let mut sound = desk.chat_sound();
                if widgets::checkbox(ui, p, &mut sound, tr!("folderstorm-chat-sound"))
                    .on_hover_text(tr!("folderstorm-chat-sound-help"))
                    .changed()
                {
                    acts.push(HostAct::ChatSound(sound));
                }
                let preview = tr!("folderstorm-settings-preview");
                if widgets::action_button(ui, p, "player-play-filled", preview, ActionKind::Flat, BUTTON)
                    .on_hover_text(tr!("folderstorm-settings-preview-help"))
                    .clicked()
                {
                    acts.push(HostAct::Settings(Act::Preview));
                }
                note(ui, p, tr!("folderstorm-settings-alert-note"));
            },
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-heard"));
                note(ui, p, tr!("folderstorm-host-sound-all-help"));
                let slots: Vec<_> = desk.slots().map(|(slot, s)| (slot, s.clone())).collect();
                if slots.is_empty() {
                    note(ui, p, tr!("folderstorm-host-empty"));
                }
                for (slot, state) in slots {
                    ui.push_id(("folderstorm-settings-heard", slot), |ui| {
                        let mut heard = state.sound == ::folderstorm_session::host::BackgroundSound::All;
                        let label = format!("{}: {}", slot + 1, super::shown_name(&state));
                        if widgets::checkbox(ui, p, &mut heard, label).changed() {
                            let sound = if heard {
                                ::folderstorm_session::host::BackgroundSound::All
                            } else {
                                ::folderstorm_session::host::BackgroundSound::Muted
                            };
                            acts.push(HostAct::Comfort(super::comfort::Act::Sound(slot, sound)));
                        }
                    });
                }
            },
        );
    }

    /// Files, windows, diagnostics, the icons' credits.
    fn advanced_settings(&mut self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<HostAct>) {
        let recovered = self.settings.recovered;
        two(
            ui,
            acts,
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-files"));
                let open = tr!("folderstorm-settings-open-folder");
                if widgets::action_button(ui, p, "folder-filled", open, ActionKind::Flat, BUTTON)
                    .on_hover_text(crate::settings::config_dir().display().to_string())
                    .clicked()
                {
                    acts.push(HostAct::Settings(Act::OpenFolder));
                }
                note(ui, p, tr!("folderstorm-settings-open-folder-help"));
            },
            |ui, acts| {
                heading(ui, p, tr!("folderstorm-settings-windows"));
                let recover = tr!("folderstorm-settings-recover");
                if widgets::action_button(ui, p, "app-window-filled", recover, ActionKind::Flat, BUTTON).clicked() {
                    acts.push(HostAct::Settings(Act::Recover));
                }
                note(ui, p, tr!("folderstorm-settings-recover-help"));
                if let Some(count) = recovered {
                    ui.label(RichText::new(tr!("folderstorm-settings-recovered", count = count)).color(p.ink));
                }
            },
        );
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-settings-diagnostics"));
        let mut diagnostics = self.settings.diagnostics;
        if widgets::checkbox(ui, p, &mut diagnostics, tr!("folderstorm-settings-show-diagnostics")).changed() {
            acts.push(HostAct::Settings(Act::Diagnostics(diagnostics)));
        }
        if diagnostics {
            self.diagnostics(ui, p);
        }
        ui.add_space(8.0);
        heading(ui, p, tr!("folderstorm-settings-credits"));
        ui.label(RichText::new(tr!("folderstorm-settings-credits-tabler")).color(p.ink));
        let mut credits = self.settings.credits;
        if widgets::checkbox(ui, p, &mut credits, tr!("folderstorm-settings-show-licence")).changed() {
            acts.push(HostAct::Settings(Act::Credits(credits)));
        }
        if credits {
            egui::Frame::new()
                .fill(p.field)
                .inner_margin(10.0)
                .corner_radius(widgets::inner_radius(ui.visuals()))
                .show(ui, |ui| {
                    ui.label(RichText::new(TABLER_LICENCE).monospace().size(11.5).color(p.muted));
                });
        }
    }

    /// What the host knows of each character, for a report.
    fn diagnostics(&self, ui: &mut egui::Ui, p: &Palette) {
        let now = std::time::Instant::now();
        let desk = &self.host.desk;
        egui::Frame::new()
            .fill(p.field)
            .inner_margin(10.0)
            .corner_radius(widgets::inner_radius(ui.visuals()))
            .show(ui, |ui| {
                let mut lines = vec![
                    format!(
                        "{} {}",
                        folderstorm_brand::PRODUCT,
                        crate::folderstorm_characters::managed::build_id()
                    ),
                    format!(
                        "{}: {}",
                        tr!("folderstorm-settings-diagnostics-protocol"),
                        ::folderstorm_session::ipc::PROTOCOL
                    ),
                    format!("{}: {}/{}", tr!("folderstorm-settings-diagnostics-running"), desk.live(), MAX_SLOTS),
                ];
                for (slot, state) in desk.slots() {
                    // (the phase in the cards' words; the rest as the code
                    // names it, for a report)
                    let screen = state.status.as_ref().map_or_else(|| "-".to_owned(), |s| format!("{:?}", s.screen));
                    let watch = format!("{:?}", desk.watch(slot));
                    lines.push(format!(
                        "{}: {} · {} · {:?} · {screen} · #{} · {}",
                        slot + 1,
                        state.profile.shown(),
                        super::status_line(state),
                        desk.health(slot, now),
                        state.launch(),
                        watch.split([' ', '{', '(']).next().unwrap_or_default()
                    ));
                }
                ui.label(RichText::new(lines.join("\n")).monospace().size(11.5).color(p.ink));
            });
    }

    /// Where the viewer's own settings for updates and support are.
    fn updates_settings(&self, ui: &mut egui::Ui, p: &Palette) {
        heading(ui, p, tr!("folderstorm-settings-updates-where"));
        for line in [
            tr!("folderstorm-settings-updates-line", page = tr!("ui-options-page-system").to_owned()),
            tr!("folderstorm-settings-crash-line", page = tr!("ui-options-privacy").to_owned()),
            tr!("folderstorm-settings-bug-line", item = tr!("bug-title").to_owned()),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(p.muted));
                ui.label(RichText::new(line).color(p.ink));
            });
        }
        ui.add_space(4.0);
        note(ui, p, tr!("folderstorm-settings-updates-note"));
    }
}

/// Where the settings window is, when that is a place to open it again.
fn window_place(view: &View) -> Option<Place> {
    let window = view.window.as_ref()?;
    if window.is_minimized().unwrap_or(false) || window.is_maximized() {
        return None;
    }
    let size = window.inner_size();
    Some(Place {
        position: window.outer_position().ok().map(|at| [at.x, at.y]),
        size: [size.width, size.height],
    })
}

#[cfg(test)]
mod tests;
