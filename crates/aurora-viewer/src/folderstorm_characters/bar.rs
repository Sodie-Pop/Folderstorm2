//! The session host's bar, drawn (session-host plan, checkpoint 8): one
//! row from which the characters are driven. The same drawing serves both
//! homes of the bar: the top of a managed viewer's window (the viewer's
//! own interface, `app/folderstorm_characters/bar.rs`) and the host's
//! compact "Character controls" window (`host/ui/bar.rs`, drawn on the
//! processor). What the row is made of, which quick actions show and how
//! a button is labelled are the `folderstorm-session` crate's (`bar`).
//!
//! In a viewer the bar has a strip of its own ([`strip_place`]): at the
//! top of the custom world viewport, as wide as the world, when the
//! viewer has one (unless the host's settings say span the window), and
//! across the top of the window otherwise. Across the window, [`inset`]
//! keeps it out of the room the interface lays itself out in and
//! [`bars_room`] puts the viewer's own bars under it; over the world, the
//! world's top moves down by the strip ([`world_under`]) and the margins
//! keep their height. [`show_strip`] draws the bar in it.
//!
//! This file only draws a [`State`] and returns the clicks as
//! [`Request`]s. It keeps nothing: the names, aliases and colours it
//! shows live in the state it is handed and go with it.
//!
//! Left to right: the characters' list, "open a login", the shared chat,
//! the monitors' menu, the settings' menu; the quick actions; then, in a
//! viewer, "pop out" and "collapse", and in the host's window the pin
//! and "dock back". A row with too little room drops its labels, then the
//! name in the characters' list, then lets the quick actions scroll; with
//! no room even for its two ends, the whole row scrolls.

use crate::theme::Palette;
use crate::ui::{
    menu,
    widgets::{self, ActionKind},
};
use ::folderstorm_session::bar::{Button, Character, LabelMode, Page, Presence, Request, State};
use ::folderstorm_session::host::Quick;
use ::folderstorm_session::identity::{Colour, TEXT_CONTRAST};
use aurora_i18n::tr;
use egui::{Color32, Vec2};

/// Where the bar is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    /// At the top of a managed viewer's window.
    Viewer,
    /// In the host's compact window; `pinned`: it stays over the others.
    Host { pinned: bool },
}

/// The height of the row's buttons, the width of one that is an icon
/// alone, and the room between two buttons and two groups.
const SIDE: f32 = 24.0;
const ICON_WIDTH: f32 = 30.0;
const GAP: f32 = 4.0;
const GROUP_GAP: f32 = 10.0;
/// The least room the quick actions keep between the row's two ends;
/// with less, the whole row scrolls.
const SCROLL_ROOM: f32 = 44.0;

/// The longest name a button shows; the rest is in its hover.
const NAME: usize = 16;
/// The longest name a row of the characters' list shows.
const LIST_NAME: usize = 32;

/// What one button of the row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Characters,
    Login,
    Chat,
    Monitors,
    Settings,
    Quick,
    PopOut,
    Collapse,
    Pin,
    Dock,
}

/// One button, before it is drawn.
struct Face {
    kind: Kind,
    icon: &'static str,
    /// Its text when the row shows texts.
    label: String,
    /// What it shows beside its icon when the row shows icons alone (a
    /// character's number, how many wait): nothing for most.
    short: String,
    tip: String,
    face: ActionKind,
    enabled: bool,
    /// A character's colour, as a line under its button.
    tint: Option<Color32>,
    /// What a click asks for; None: the button opens a menu.
    ask: Option<Request>,
}

impl Face {
    /// The icon and the text it shows with `mode`.
    fn shown(&self, mode: LabelMode) -> (&'static str, &str) {
        let icon = if mode.icon() { self.icon } else { "" };
        let text = if mode.text() { self.label.as_str() } else { self.short.as_str() };
        (icon, text)
    }

    fn width(&self, ui: &egui::Ui, mode: LabelMode) -> f32 {
        let (icon, text) = self.shown(mode);
        match self.kind {
            // (with its chevron: `widgets::action_menu_button`)
            Kind::Characters => widgets::action_button_fit(ui, icon, text) + 16.0,
            _ if text.is_empty() => ICON_WIDTH,
            _ => widgets::action_button_fit(ui, icon, text),
        }
    }

    fn draw(&self, ui: &mut egui::Ui, p: &Palette, mode: LabelMode) -> egui::Response {
        let (icon, text) = self.shown(mode);
        let response = match self.kind {
            Kind::Characters => widgets::action_menu_button(ui, p, icon, text, false, SIDE),
            _ => {
                let width = if text.is_empty() { ICON_WIDTH } else { 0.0 };
                widgets::action_button_if(ui, p, icon, text, self.face, Vec2::new(width, SIDE), self.enabled)
            }
        };
        if let Some(tint) = self.tint {
            underline(ui, response.rect, tint);
        }
        response.on_hover_text(&self.tip)
    }
}

/// A colour as this theme draws it: readable on the bar's background and
/// on a button's face.
fn tint(p: &Palette, colour: Colour) -> Option<Color32> {
    let rgb = |c: Color32| [c.r(), c.g(), c.b()];
    let [r, g, b] = colour.fitted(&[rgb(p.panel), rgb(p.field), rgb(p.raised)], TEXT_CONTRAST)?;
    Some(Color32::from_rgb(r, g, b))
}

/// A character's colour as a line under its button.
fn underline(ui: &egui::Ui, rect: egui::Rect, tint: Color32) {
    let line = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 5.0, rect.bottom() - 3.0),
        egui::pos2(rect.right() - 5.0, rect.bottom() - 1.0),
    );
    ui.painter().rect_filled(line, widgets::inner_radius(ui.visuals()), tint);
}

/// A name cut to `max` characters, with an ellipsis when it was cut.
fn cut(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_owned();
    }
    let mut short: String = name.chars().take(max.saturating_sub(1)).collect();
    short.push('…');
    short
}

/// A character's number and name, as its rows and buttons say it.
fn numbered(character: &Character, max: usize) -> String {
    format!("{}: {}", character.slot + 1, cut(&character.name, max))
}

/// A character's whole name for a hover: its number, its name, and the
/// real one behind an alias.
fn full_name(character: &Character) -> String {
    match &character.real {
        Some(real) => format!("{}: {} ({real})", character.slot + 1, character.name),
        None => format!("{}: {}", character.slot + 1, character.name),
    }
}

/// What picking a character does, as its icon and its hover.
fn quick_face(quick: Quick, name: String) -> (&'static str, String) {
    match quick {
        Quick::Switch => ("arrows-exchange", tr!("folderstorm-quick-switch-help", name = name)),
        Quick::Show => ("eye-filled", tr!("folderstorm-quick-show-help", name = name)),
        Quick::LogIn => ("login-2", tr!("folderstorm-quick-login-help", name = name)),
        Quick::Wait => ("hourglass-filled", tr!("folderstorm-quick-wait-help", name = name)),
        Quick::Launch => ("player-play-filled", tr!("folderstorm-quick-launch-help", name = name)),
        Quick::Restart => ("refresh", tr!("folderstorm-quick-restart-help", name = name)),
        Quick::Nothing => ("x", tr!("folderstorm-quick-nothing-help", name = name)),
    }
}

/// Where a character is, in a word; empty when it is not logged in.
fn presence_text(presence: Presence) -> &'static str {
    match presence {
        Presence::Active => tr!("folderstorm-host-active"),
        Presence::Inactive => tr!("folderstorm-bar-inactive"),
        Presence::Out => "",
    }
}

/// The toolbar's buttons at the left of the row. `tight`: the row has no
/// room for the active character's name.
fn left_faces(state: &State, tight: bool) -> Vec<Face> {
    let plain = |kind, icon, label: &str, tip: String| Face {
        kind,
        icon,
        label: label.to_owned(),
        short: String::new(),
        tip,
        face: ActionKind::Flat,
        enabled: true,
        tint: None,
        ask: None,
    };
    let active = state.active();
    // (the list always says who is in front, whatever the labels)
    let who = match (active, tight) {
        (Some(active), false) => numbered(active, NAME),
        (Some(active), true) => (active.slot + 1).to_string(),
        (None, false) => tr!("folderstorm-bar-characters").to_owned(),
        (None, true) => String::new(),
    };
    let characters = Face {
        label: who.clone(),
        short: who,
        ..plain(
            Kind::Characters,
            "users",
            "",
            match active {
                Some(active) => format!("{}\n{}", full_name(active), tr!("folderstorm-bar-characters-help")),
                None => tr!("folderstorm-bar-characters-help").to_owned(),
            },
        )
    };
    let next = state.next_login();
    let login = Face {
        enabled: next.is_some(),
        ask: next.map(|_| Request::OpenLogin(None)),
        ..plain(
            Kind::Login,
            "login-2",
            tr!("folderstorm-bar-login"),
            match next {
                Some(next) => tr!("folderstorm-bar-login-help", name = full_name(next)),
                None => tr!("folderstorm-bar-login-none").to_owned(),
            },
        )
    };
    let unread = state.chat_unread;
    let chat = Face {
        short: if unread > 0 { count_text(unread) } else { String::new() },
        label: match unread {
            0 => tr!("folderstorm-bar-chat").to_owned(),
            unread => format!("{} · {}", tr!("folderstorm-bar-chat"), count_text(unread)),
        },
        face: if state.chat_open { ActionKind::Primary } else { ActionKind::Flat },
        ask: Some(Request::Chat(!state.chat_open)),
        ..plain(Kind::Chat, "messages-filled", "", {
            let mut tip = if state.chat_open {
                tr!("folderstorm-bar-chat-hide-help").to_owned()
            } else {
                tr!("folderstorm-bar-chat-show-help").to_owned()
            };
            if unread > 0 {
                tip.push('\n');
                tip.push_str(&tr!("folderstorm-bar-chat-unread", count = unread));
            }
            tip
        })
    };
    let monitors = plain(
        Kind::Monitors,
        "device-desktop-filled",
        tr!("folderstorm-bar-monitors"),
        tr!("folderstorm-bar-monitors-help").to_owned(),
    );
    let settings = plain(
        Kind::Settings,
        "settings-filled",
        tr!("folderstorm-bar-settings"),
        tr!("folderstorm-bar-settings-help").to_owned(),
    );
    vec![characters, login, chat, monitors, settings]
}

/// A count as a button shows it.
fn count_text(count: u32) -> String {
    if count > 99 { "99+".to_owned() } else { count.to_string() }
}

/// The quick actions in the middle of the row.
fn quick_faces(p: &Palette, state: &State) -> Vec<Face> {
    state
        .buttons()
        .into_iter()
        .map(|button| match button {
            Button::Character(character) => {
                let (icon, mut tip) = quick_face(character.quick, character.name.clone());
                if let Some(real) = &character.real {
                    // (the name behind the alias)
                    tip.push('\n');
                    tip.push_str(real);
                }
                Face {
                    kind: Kind::Quick,
                    icon,
                    label: format!("{} · {}", character.slot + 1, cut(&character.name, NAME)),
                    short: (character.slot + 1).to_string(),
                    tip,
                    face: if character.presence == Presence::Active {
                        ActionKind::Primary
                    } else {
                        ActionKind::Flat
                    },
                    enabled: character.quick != Quick::Nothing,
                    tint: tint(p, character.colour),
                    ask: Some(Request::Pick(character.slot)),
                }
            }
            Button::Attention(count) => Face {
                kind: Kind::Quick,
                icon: "bell-filled",
                label: match count {
                    0 => tr!("folderstorm-bar-attention").to_owned(),
                    count => format!("{} · {}", tr!("folderstorm-bar-attention"), count_text(count)),
                },
                short: if count > 0 { count_text(count) } else { String::new() },
                tip: tr!("folderstorm-bar-attention-help", count = count),
                face: if count > 0 { ActionKind::Caution } else { ActionKind::Flat },
                enabled: true,
                tint: None,
                ask: Some(Request::Inbox),
            },
            Button::MuteBackground(on) => Face {
                kind: Kind::Quick,
                icon: if on { "volume-off" } else { "volume" },
                label: tr!("folderstorm-bar-mute").to_owned(),
                short: String::new(),
                tip: if on {
                    tr!("folderstorm-bar-mute-on-help").to_owned()
                } else {
                    tr!("folderstorm-bar-mute-off-help").to_owned()
                },
                face: if on { ActionKind::Primary } else { ActionKind::Flat },
                enabled: true,
                tint: None,
                ask: Some(Request::MuteBackground(!on)),
            },
            Button::VoiceFollows(on) => Face {
                kind: Kind::Quick,
                icon: "microphone-filled",
                label: tr!("folderstorm-bar-voice").to_owned(),
                short: String::new(),
                tip: if on {
                    tr!("folderstorm-bar-voice-on-help").to_owned()
                } else {
                    tr!("folderstorm-bar-voice-off-help").to_owned()
                },
                face: if on { ActionKind::Primary } else { ActionKind::Flat },
                enabled: true,
                tint: None,
                ask: Some(Request::VoiceFollows(!on)),
            },
        })
        .collect()
}

/// The buttons at the right end of the row, from the left.
fn right_faces(home: Home) -> Vec<Face> {
    let plain = |kind, icon, label: &str, tip: &str, ask| Face {
        kind,
        icon,
        label: label.to_owned(),
        short: String::new(),
        tip: tip.to_owned(),
        face: ActionKind::Flat,
        enabled: true,
        tint: None,
        ask: Some(ask),
    };
    match home {
        Home::Viewer => vec![
            plain(
                Kind::PopOut,
                "external-link",
                tr!("folderstorm-bar-pop-out"),
                tr!("folderstorm-bar-pop-out-help"),
                Request::Dock(false),
            ),
            plain(
                Kind::Collapse,
                "layout-navbar-collapse-filled",
                tr!("folderstorm-bar-collapse"),
                tr!("folderstorm-bar-collapse-help"),
                Request::Collapse(true),
            ),
        ],
        Home::Host { pinned } => vec![
            Face {
                // pinned: an upright pin on the accent; not pinned: a
                // plain, tilted one
                face: if pinned { ActionKind::Primary } else { ActionKind::Flat },
                ..plain(
                    Kind::Pin,
                    if pinned { "pinned-filled" } else { "pin-filled" },
                    if pinned {
                        tr!("folderstorm-bar-pinned")
                    } else {
                        tr!("folderstorm-bar-pin")
                    },
                    if pinned {
                        tr!("folderstorm-bar-pinned-help")
                    } else {
                        tr!("folderstorm-bar-pin-help")
                    },
                    Request::Pin(!pinned),
                )
            },
            plain(
                Kind::Dock,
                "arrow-back-up",
                tr!("folderstorm-bar-dock"),
                tr!("folderstorm-bar-dock-help"),
                Request::Dock(true),
            ),
        ],
    }
}

fn row_width(ui: &egui::Ui, faces: &[Face], mode: LabelMode) -> f32 {
    faces.iter().map(|face| face.width(ui, mode) + GAP).sum()
}

/// The list of the characters: each with where it is; picking one is its
/// quick action.
fn characters_menu(ui: &mut egui::Ui, p: &Palette, state: &State, asked: &mut Vec<Request>) {
    if state.characters.is_empty() {
        menu::info(ui, p, "users", tr!("folderstorm-bar-characters-none"));
    }
    for character in &state.characters {
        let (icon, tip) = quick_face(character.quick, full_name(character));
        let label = numbered(character, LIST_NAME);
        let clicked = if character.quick == Quick::Nothing {
            menu::item_tip(ui, p, icon, &label, false, &tip)
        } else {
            menu::item_detail(ui, p, icon, &label, presence_text(character.presence))
        };
        if clicked {
            asked.push(Request::Pick(character.slot));
        }
    }
}

/// "Open a login", by a right click: which character.
fn login_menu(ui: &mut egui::Ui, p: &Palette, state: &State, asked: &mut Vec<Request>) {
    let mut none = true;
    for character in state.logins() {
        none = false;
        if menu::item(ui, p, "login-2", &numbered(character, LIST_NAME)) {
            asked.push(Request::OpenLogin(Some(character.slot)));
        }
    }
    if none {
        menu::info(ui, p, "login-2", tr!("folderstorm-bar-login-none"));
    }
}

fn monitors_menu(ui: &mut egui::Ui, p: &Palette, state: &State, asked: &mut Vec<Request>) {
    for character in &state.characters {
        // greyed when it is not in the world, or is the one in front
        let label = numbered(character, LIST_NAME);
        if menu::check(ui, p, "device-desktop-filled", &label, character.monitor, character.watchable()) {
            asked.push(Request::Monitor(character.slot, !character.monitor));
        }
    }
    if !state.characters.is_empty() {
        menu::separator(ui, p);
    }
    let closed = state.characters.iter().any(|c| c.watchable() && !c.monitor);
    if menu::item_if(ui, p, "device-desktop-filled", tr!("folderstorm-bar-monitors-open-all"), closed) {
        asked.push(Request::OpenMonitors);
    }
    // (its place: the cycling monitor comes with the monitors' own windows)
    menu::item_tip(
        ui,
        p,
        "refresh",
        tr!("folderstorm-bar-monitors-cycling"),
        false,
        tr!("folderstorm-bar-monitors-cycling-help"),
    );
    let open = state.characters.iter().any(|c| c.monitor);
    if menu::item_if(ui, p, "x", tr!("folderstorm-bar-monitors-close-all"), open) {
        asked.push(Request::CloseMonitors);
    }
    menu::separator(ui, p);
    if menu::item(ui, p, "settings-filled", tr!("folderstorm-bar-monitors-settings")) {
        asked.push(Request::Show(Page::Monitors));
    }
}

fn settings_menu(ui: &mut egui::Ui, p: &Palette, state: &State, asked: &mut Vec<Request>) {
    if menu::item(ui, p, "settings-filled", tr!("folderstorm-bar-settings-bar")) {
        asked.push(Request::Show(Page::Bar));
    }
    if menu::item(ui, p, "keyboard-filled", tr!("folderstorm-host-hotkeys")) {
        asked.push(Request::Show(Page::Hotkeys));
    }
    menu::separator(ui, p);
    if menu::check(ui, p, "volume-off", tr!("folderstorm-bar-mute-long"), state.mute_background, true) {
        asked.push(Request::MuteBackground(!state.mute_background));
    }
    if menu::check(
        ui,
        p,
        "microphone-filled",
        tr!("folderstorm-bar-voice-long"),
        state.voice_follows,
        true,
    ) {
        asked.push(Request::VoiceFollows(!state.voice_follows));
    }
    menu::separator(ui, p);
    if menu::item(ui, p, "app-window-filled", tr!("folderstorm-bar-show-host")) {
        asked.push(Request::Show(Page::Host));
    }
    if menu::item(ui, p, "power", tr!("folderstorm-bar-quit")) {
        asked.push(Request::Show(Page::Quit));
    }
}

/// Draws a button and what it opens; its click, if any, is asked.
fn place(ui: &mut egui::Ui, p: &Palette, state: &State, face: &Face, mode: LabelMode, asked: &mut Vec<Request>) {
    let response = face.draw(ui, p, mode);
    let close = egui::PopupCloseBehavior::CloseOnClick;
    match face.kind {
        Kind::Characters => {
            menu::dropdown(&response, p, close, |ui| characters_menu(ui, p, state, asked));
        }
        Kind::Monitors => {
            menu::dropdown(&response, p, close, |ui| monitors_menu(ui, p, state, asked));
        }
        Kind::Settings => {
            menu::dropdown(&response, p, close, |ui| settings_menu(ui, p, state, asked));
        }
        Kind::Login => {
            menu::context_menu(&response, p, |ui| login_menu(ui, p, state, asked));
        }
        Kind::Chat | Kind::Quick | Kind::PopOut | Kind::Collapse | Kind::Pin | Kind::Dock => {}
    }
    if let Some(ask) = face.ask
        && face.enabled
        && response.clicked()
    {
        asked.push(ask);
    }
}

/// A collapsed bar: a thin handle across the window that opens it again.
fn handle(ui: &mut egui::Ui, p: &Palette, asked: &mut Vec<Request>) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
    let hot = response.hovered();
    if hot {
        ui.painter().rect_filled(rect, 0.0, p.raised);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let grip = egui::Rect::from_center_size(rect.center(), Vec2::new(36.0, 2.0));
    ui.painter()
        .rect_filled(grip, widgets::inner_radius(ui.visuals()), if hot { p.ink } else { p.muted_dim });
    if response.on_hover_text(tr!("folderstorm-bar-expand-help")).clicked() {
        asked.push(Request::Collapse(false));
    }
}

/// Draws the bar in `ui`, whose room is the row, and returns what was
/// clicked. The caller paints the row's background.
pub fn show(ui: &mut egui::Ui, p: &Palette, state: &State, home: Home) -> Vec<Request> {
    let mut asked = Vec::new();
    if home == Home::Viewer && state.collapsed {
        handle(ui, p, &mut asked);
        return asked;
    }
    let quick = quick_faces(p, state);
    let right = right_faces(home);
    let room = ui.available_width();
    // with too little room: the toolbar's labels go, then the quick
    // actions', then the active character's name; what is left of the
    // quick actions scrolls
    let choices = [
        (state.toolbar_labels, state.quick_labels, false),
        (state.toolbar_labels.narrow(), state.quick_labels, false),
        (state.toolbar_labels.narrow(), state.quick_labels.narrow(), false),
        (state.toolbar_labels.narrow(), state.quick_labels.narrow(), true),
    ];
    let mut chosen = choices[choices.len() - 1];
    for choice in choices {
        let (toolbar, labels, tight) = choice;
        let left = left_faces(state, tight);
        let needed = row_width(ui, &left, toolbar) + row_width(ui, &right, toolbar) + row_width(ui, &quick, labels) + 2.0 * GROUP_GAP;
        if needed <= room {
            chosen = choice;
            break;
        }
    }
    let (toolbar, labels, tight) = chosen;
    let left = left_faces(state, tight);
    ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
    // (the wheel moves a row that only scrolls sideways)
    ui.style_mut().always_scroll_the_only_direction = true;
    let scrolled = |salt: &'static str| {
        egui::ScrollArea::horizontal()
            .id_salt(salt)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
    };
    let ends = row_width(ui, &left, toolbar) + row_width(ui, &right, toolbar) + 2.0 * GROUP_GAP;
    if ends + SCROLL_ROOM > room {
        // not even the two ends fit: the whole row, in its order, scrolls
        // (nothing is laid over anything else)
        scrolled("folderstorm-bar-row").show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                for face in &left {
                    place(ui, p, state, face, toolbar, &mut asked);
                }
                ui.add_space(GROUP_GAP - GAP);
                for face in &quick {
                    place(ui, p, state, face, labels, &mut asked);
                }
                ui.add_space(GROUP_GAP - GAP);
                for face in &right {
                    place(ui, p, state, face, toolbar, &mut asked);
                }
            });
        });
        return asked;
    }
    ui.horizontal_centered(|ui| {
        for face in &left {
            place(ui, p, state, face, toolbar, &mut asked);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for face in right.iter().rev() {
                place(ui, p, state, face, toolbar, &mut asked);
            }
            ui.add_space(GROUP_GAP - GAP);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(GROUP_GAP - GAP);
                scrolled("folderstorm-bar-quick").show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        for face in &quick {
                            place(ui, p, state, face, labels, &mut asked);
                        }
                    });
                });
            });
        });
    });
    asked
}

/// Where a viewer's strip is this frame ([`strip_place`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strip {
    /// Across the top of the window, this many points high: the
    /// interface's pass keeps it out of its room ([`inset`]), so the
    /// screens, the windows and the viewer's bars start under it.
    Window(f32),
    /// Over the custom world viewport only: the strip's rectangle in
    /// window pixels (x, y, width, height), at the world's top and as wide
    /// as the world. The world starts under it ([`world_under`]); the
    /// rest of the window, where windows sit in the margins, keeps its
    /// whole height.
    World([u32; 4]),
}

/// Where the strip of a viewer goes: over the world viewport when the
/// viewer has one this frame (`world`, from `folderstorm::viewport`, in
/// window pixels), its own bars stay inside it (`bars_in_viewport`, the
/// viewport's "Keep the bars inside the viewport") and the host did not
/// ask for the whole window ([`State::span_window`]); across the window
/// otherwise, as without a custom viewport. None without a strip
/// (popped out). A world too low to keep anything under the strip gets
/// the window's strip.
pub fn strip_place(state: &State, world: Option<[u32; 4]>, bars_in_viewport: bool, pixels_per_point: f32) -> Option<Strip> {
    let height = state.strip();
    if height <= 0.0 {
        return None;
    }
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let over = world.filter(|_| bars_in_viewport && !state.span_window).and_then(|[x, y, w, h]| {
        let strip = ((height * ppp).round() as u32).max(1);
        (strip < h && w > 0).then_some([x, y, w, strip])
    });
    Some(over.map_or(Strip::Window(height), Strip::World))
}

/// The world viewport under a strip over it ([`Strip::World`]): the same
/// width, its top moved down by the strip.
pub fn world_under(world: [u32; 4], strip: [u32; 4]) -> [u32; 4] {
    let [x, y, w, h] = world;
    let cut = strip[3].min(h.saturating_sub(1));
    [x, y.saturating_add(cut), w, h - cut]
}

/// A window of `rect` points in pixels, as the renderer has it.
pub fn window_pixels(rect: egui::Rect, pixels_per_point: f32) -> (u32, u32) {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let size = rect.size() * ppp;
    (size.x.max(0.0).round() as u32, size.y.max(0.0).round() as u32)
}

/// The inset a viewer's interface pass is given for the bar: egui takes
/// it out of the room the interface lays itself out in
/// (`Context::content_rect`). The strip's height across the window;
/// nothing for a strip over the world viewport only (the world itself
/// moves down: [`world_under`]) or without a bar.
pub fn inset(strip: Option<Strip>) -> egui::SafeAreaInsets {
    egui::SafeAreaInsets(egui::epaint::MarginF32 {
        top: match strip {
            Some(Strip::Window(height)) => height,
            _ => 0.0,
        },
        ..Default::default()
    })
}

/// The room the viewer's own bars are laid out in, under the bar's strip:
/// `bars` (the custom world viewport's, if any: `folderstorm::viewport`)
/// as it is when the pass kept no strip, so a viewer without a bar, or
/// with the bar over the world viewport (whose top is already under it),
/// is as it was; otherwise the interface's room, or what of `bars` is in
/// it. (egui's root `Ui` spans the whole window whatever the inset: the
/// viewer's bars, which are laid out in it, are given this instead.)
pub fn bars_room(ctx: &egui::Context, bars: Option<egui::Rect>) -> Option<egui::Rect> {
    let (window, room) = (ctx.viewport_rect(), ctx.content_rect());
    if room == window {
        return bars;
    }
    Some(bars.map_or(room, |bars| bars.intersect(room)))
}

/// The strip's rectangle in points: what the window has above the
/// interface's room ([`Strip::Window`]), or the rectangle over the world
/// viewport.
fn strip_rect(ctx: &egui::Context, strip: Strip) -> egui::Rect {
    let (window, room) = (ctx.viewport_rect(), ctx.content_rect());
    match strip {
        Strip::Window(_) => egui::Rect::from_min_max(window.min, egui::pos2(window.right(), room.top())),
        Strip::World([x, y, w, h]) => {
            let ppp = ctx.pixels_per_point();
            let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
            egui::Rect::from_min_size(
                window.min + egui::vec2(x as f32 / ppp, y as f32 / ppp),
                egui::vec2(w as f32 / ppp, h as f32 / ppp),
            )
            .intersect(window)
        }
    }
}

/// Draws the bar of a viewer in its strip ([`strip_place`]). Nothing when
/// the pass kept no room for it.
pub fn show_strip(ctx: &egui::Context, p: &Palette, state: &State, strip: Strip) -> Vec<Request> {
    let window = ctx.viewport_rect();
    let strip = strip_rect(ctx, strip);
    if strip.height() < 1.0 || strip.width() < 1.0 {
        return Vec::new();
    }
    // an area of its own in the windows' order, as the viewer's islands
    // are: the pointer over it is then over the interface
    // (`Context::is_pointer_over_egui`), and a click on the bar is never
    // also a click in the world behind it
    egui::Area::new(egui::Id::unique("folderstorm-character-bar"))
        .order(egui::Order::Middle)
        .fixed_pos(strip.min)
        .constrain_to(window)
        .movable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(strip);
            ui.set_min_size(strip.size());
            show_in(ui, p, strip, state, Home::Viewer)
        })
        .inner
}

/// The bar in a rectangle of an interface, over a plain background: the
/// top strip of a viewer's window, or the row of the host's compact
/// window.
pub fn show_in(ui: &mut egui::Ui, p: &Palette, rect: egui::Rect, state: &State, home: Home) -> Vec<Request> {
    ui.painter().rect_filled(rect, 0.0, p.panel);
    let collapsed = home == Home::Viewer && state.collapsed;
    let inside = if collapsed {
        rect
    } else {
        rect.shrink2(Vec2::new(6.0, ((rect.height() - SIDE) / 2.0).max(0.0)))
    };
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inside)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.set_clip_rect(rect.intersect(ui.clip_rect()));
    show(&mut row, p, state, home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::folderstorm_session::bar::{Config, QuickAction};

    fn character(slot: u8, name: &str, presence: Presence, quick: Quick) -> Character {
        Character {
            slot,
            name: name.to_owned(),
            real: None,
            colour: Colour::None,
            presence,
            quick,
            monitor: false,
        }
    }

    fn sample() -> State {
        State {
            docked: true,
            characters: vec![
                Character {
                    real: Some("Jane Doe".to_owned()),
                    colour: Colour::Teal,
                    ..character(0, "Main", Presence::Active, Quick::Show)
                },
                Character {
                    colour: Colour::Orange,
                    monitor: true,
                    ..character(1, "Shop keeper", Presence::Inactive, Quick::Switch)
                },
                character(2, "A very long profile name that goes on", Presence::Out, Quick::LogIn),
                character(3, "demo-4", Presence::Out, Quick::Launch),
            ],
            inbox: 3,
            chat_open: false,
            chat_unread: 12,
            mute_background: false,
            voice_follows: true,
            quick: {
                let mut config = Config::default();
                config.set_quick(6, QuickAction::MuteBackground);
                config.set_quick(7, QuickAction::VoiceFollows);
                config.quick
            },
            quick_labels: LabelMode::IconAndText,
            toolbar_labels: LabelMode::IconOnly,
            ..State::default()
        }
    }

    #[test]
    fn the_strip_is_the_bar_s_height_and_nothing_without_one() {
        use ::folderstorm_session::bar::{HANDLE, HEIGHT};
        let top = |state: Option<&State>| inset(state.and_then(|state| strip_place(state, None, true, 1.0))).0.top;
        assert_eq!(top(None), 0.0);
        assert_eq!(top(Some(&sample())), HEIGHT);
        let collapsed = State {
            collapsed: true,
            ..sample()
        };
        assert_eq!(top(Some(&collapsed)), HANDLE);
        let popped = State { docked: false, ..sample() };
        assert_eq!(top(Some(&popped)), 0.0);
        let all = inset(Some(Strip::Window(HEIGHT))).0;
        assert_eq!((all.left, all.right, all.bottom), (0.0, 0.0, 0.0));
    }

    #[test]
    fn the_viewer_s_bars_go_under_the_strip_and_are_left_alone_without_one() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let viewport = egui::Rect::from_min_max(egui::pos2(400.0, 0.0), egui::pos2(900.0, 720.0));
        let pass = |state: Option<&State>, check: &mut dyn FnMut(&egui::Context)| {
            let ctx = egui::Context::default();
            let raw = egui::RawInput {
                screen_rect: Some(window),
                safe_area_insets: Some(inset(state.and_then(|state| strip_place(state, None, true, 1.0)))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| check(ui.ctx()));
            // (nothing is painted here)
            output.textures_delta.clear();
        };
        // no bar: whatever the bars had, they keep (None: the whole window)
        pass(None, &mut |ctx| {
            assert_eq!(bars_room(ctx, None), None);
            assert_eq!(bars_room(ctx, Some(viewport)), Some(viewport));
            let p = crate::theme::Theme::default().skin_palette();
            assert!(show_strip(ctx, &p, &sample(), Strip::Window(30.0)).is_empty());
        });
        // a bar: the room under the strip, and a custom viewport's part of it
        let state = sample();
        pass(Some(&state), &mut |ctx| {
            let room = bars_room(ctx, None).expect("a room");
            assert_eq!((room.top(), room.bottom(), room.width()), (30.0, 720.0, 1280.0));
            let inside = bars_room(ctx, Some(viewport)).expect("a room");
            assert_eq!((inside.left(), inside.right(), inside.top()), (400.0, 900.0, 30.0));
        });
    }

    #[test]
    fn the_strip_is_over_the_world_viewport_unless_asked_to_span_the_window() {
        use ::folderstorm_session::bar::{HANDLE, HEIGHT};
        let state = sample();
        // two 1920-pixel monitors, the world on the right one (a left
        // margin) or on the left one (a right margin)
        let right_one = Some([1920, 0, 1920, 1080]);
        let left_one = Some([0, 0, 1920, 1080]);
        assert_eq!(strip_place(&state, right_one, true, 1.0), Some(Strip::World([1920, 0, 1920, 30])));
        assert_eq!(strip_place(&state, left_one, true, 1.0), Some(Strip::World([0, 0, 1920, 30])));
        // a top margin too: the strip is at the world's top
        assert_eq!(
            strip_place(&state, Some([1920, 100, 1920, 980]), true, 1.0),
            Some(Strip::World([1920, 100, 1920, 30]))
        );
        // in pixels: the interface's scale
        assert_eq!(strip_place(&state, right_one, true, 1.5), Some(Strip::World([1920, 0, 1920, 45])));
        let collapsed = State {
            collapsed: true,
            ..sample()
        };
        assert_eq!(strip_place(&collapsed, left_one, true, 2.0), Some(Strip::World([0, 0, 1920, 16])));
        // the option: across the whole window, as without a viewport
        let span = State {
            span_window: true,
            ..sample()
        };
        assert_eq!(strip_place(&span, right_one, true, 1.0), Some(Strip::Window(HEIGHT)));
        assert_eq!(strip_place(&span, left_one, true, 1.0), Some(Strip::Window(HEIGHT)));
        // no custom viewport (or none this frame): across the window,
        // whatever the option
        assert_eq!(strip_place(&state, None, true, 1.0), Some(Strip::Window(HEIGHT)));
        assert_eq!(strip_place(&span, None, true, 1.0), Some(Strip::Window(HEIGHT)));
        assert_eq!(strip_place(&collapsed, None, true, 1.0), Some(Strip::Window(HANDLE)));
        // the viewer's bars over the whole window: the bar with them
        assert_eq!(strip_place(&state, right_one, false, 1.0), Some(Strip::Window(HEIGHT)));
        // a world too low for anything under the strip; popped out
        assert_eq!(strip_place(&state, Some([0, 0, 800, 30]), true, 1.0), Some(Strip::Window(HEIGHT)));
        let popped = State { docked: false, ..sample() };
        assert_eq!(strip_place(&popped, right_one, true, 1.0), None);
        assert_eq!(strip_place(&popped, None, true, 1.0), None);
        // the world moves down by the strip, keeping its width
        assert_eq!(world_under([1920, 0, 1920, 1080], [1920, 0, 1920, 30]), [1920, 30, 1920, 1050]);
        assert_eq!(world_under([0, 100, 1920, 980], [0, 100, 1920, 45]), [0, 145, 1920, 935]);
        assert_eq!(world_under([0, 0, 800, 20], [0, 0, 800, 30]), [0, 19, 800, 1], "never empty");
        assert_eq!(
            window_pixels(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 720.0)), 1.5),
            (1920, 1080)
        );
    }

    /// A viewer window over two monitors (2 x 1280 points), the world on
    /// one of them, Inventory in the other one's margin: over the world
    /// only, the strip is the world's width at its top, the world and its
    /// bars move down under it and the margin keeps the window's whole
    /// height; spanning, as before checkpoint 8's follow-up.
    #[test]
    fn over_the_world_the_margins_keep_their_height_and_the_world_moves_down() {
        use crate::folderstorm::viewport::{self, Overrides, ViewportSettings};
        let p = crate::theme::Theme::default().skin_palette();
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(2560.0, 720.0));
        for (left, right) in [(1280, 0), (0, 1280)] {
            let settings = ViewportSettings {
                enabled: true,
                left,
                right,
                ..Default::default()
            };
            let whole = viewport::world_rect((2560, 720), &settings, Overrides::default()).expect("a world");
            for span_window in [false, true] {
                let state = State { span_window, ..sample() };
                let strip = strip_place(&state, Some(whole), settings.bars_in_viewport, 1.0).expect("a strip");
                // what `App::world_viewport` gives the renderer
                let world = match strip {
                    Strip::World(strip) => world_under(whole, strip),
                    Strip::Window(_) => whole,
                };
                let ctx = egui::Context::default();
                let (mut content, mut bars, mut screen, mut room) = (egui::Rect::NOTHING, None, egui::Rect::NOTHING, egui::Rect::NOTHING);
                for _ in 0..2 {
                    let raw = egui::RawInput {
                        screen_rect: Some(window),
                        safe_area_insets: Some(inset(Some(strip))),
                        events: vec![egui::Event::PointerMoved(egui::pos2(left as f32 + 20.0, 15.0))],
                        ..Default::default()
                    };
                    let mut output = ctx.run_ui(raw, |ui| {
                        let ctx = ui.ctx();
                        content = ctx.content_rect();
                        // the two lines of app.rs where the bars get their room
                        bars = bars_room(ctx, viewport::bars_rect(&settings, Some(world), 1.0));
                        viewport::set_bars_area(ctx, bars);
                        screen = viewport::bars_screen(ctx);
                        room = viewport::floater_room(ctx, egui::Rect::from_min_size(egui::pos2(8.0, 48.0), Vec2::splat(300.0)));
                        show_strip(ctx, &p, &state, strip);
                    });
                    output.textures_delta.clear();
                }
                let drawn = ctx
                    .memory(|m| m.area_rect(egui::Id::unique("folderstorm-character-bar")))
                    .expect("the bar is drawn");
                let world_x = left as f32;
                if span_window {
                    assert_eq!(strip, Strip::Window(30.0));
                    assert_eq!(world, whole, "the world under the strip, as before");
                    assert_eq!(content.top(), 30.0, "the whole interface under the strip");
                    assert_eq!((drawn.left(), drawn.width()), (0.0, 2560.0));
                } else {
                    assert_eq!(strip, Strip::World([left, 0, 1280, 30]));
                    assert_eq!(world, [left, 30, 1280, 690], "the world moves down by the strip");
                    assert_eq!(content, window, "a margin window keeps the window's whole height");
                    assert_eq!(room.height(), 720.0, "and may be as high as the window");
                    assert_eq!((drawn.left(), drawn.top(), drawn.width()), (world_x, 0.0, 1280.0));
                    assert!(drawn.height() <= 30.5, "{drawn:?}");
                }
                // the viewer's bars: inside the world, under the strip, and
                // `bars_screen` agrees with where they were laid out
                let bars = bars.expect("confined to the world");
                assert_eq!(
                    (bars.left(), bars.top(), bars.width(), bars.bottom()),
                    (world_x, 30.0, 1280.0, 720.0)
                );
                assert_eq!(screen, bars);
                // the pointer on the bar is over the interface
                assert!(ctx.is_pointer_over_egui());
            }
        }
    }

    /// The picture of a viewer's window over two monitors, the bar over
    /// the world on the right one: `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>`
    /// keeps it as `<folder>/viewer-bar-over-world.png`.
    #[test]
    fn the_bar_over_the_world_is_drawn_in_the_world_s_strip_only() {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        let _icons = crate::ui::icons::Icons::load(&ctx, None);
        let (width, height) = (1600u32, 400u32);
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width as f32, height as f32));
        let state = sample();
        let strip = strip_place(&state, Some([800, 0, 800, 400]), true, 1.0).expect("a strip");
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut painter = crate::crash::software::Painter::default();
        for _ in 0..3 {
            let raw = egui::RawInput {
                screen_rect: Some(window),
                safe_area_insets: Some(inset(Some(strip))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                // the world (a plain colour here), under the strip
                let world = egui::Rect::from_min_max(egui::pos2(800.0, 30.0), egui::pos2(1600.0, 400.0));
                ui.painter().rect_filled(world, 0.0, Color32::from_rgb(40, 90, 140));
                show_strip(ui.ctx(), &p, &state, strip);
            });
            let primitives = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            pixels.fill(0);
            painter.paint(
                &mut pixels,
                [width as usize, height as usize],
                &primitives,
                &output.textures_delta,
                output.pixels_per_point,
                p.panel,
            );
            output.textures_delta.clear();
        }
        let at = |x: u32, y: u32| pixels[(y * width + x) as usize];
        // the margin's top is the plain background, the world's top is the bar
        let background = at(10, 390);
        assert!(
            (0..800).all(|x| (0..30).all(|y| at(x, y) == background)),
            "nothing of the bar in the margin"
        );
        let bar = (800..1600)
            .flat_map(|x| (0..30).map(move |y| (x, y)))
            .filter(|&(x, y)| at(x, y) != background)
            .count();
        assert!(bar > 800 * 30 / 2, "the bar fills the world's strip: {bar}");
        assert_eq!(at(1200, 200) & 0x00ff_ffff, 0x0028_5a8c, "the world under it");
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join("viewer-bar-over-world.png");
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
    }

    /// The pointer over the strip is over the interface, wherever in it
    /// (a click between two buttons is not a click in the world); under
    /// the strip, with nothing of the viewer's there, it is over the
    /// world.
    #[test]
    fn the_strip_takes_the_pointer_and_the_world_under_it_does_not() {
        let p = crate::theme::Theme::default().skin_palette();
        let over = |state: &State, at: egui::Pos2| {
            let ctx = egui::Context::default();
            for _ in 0..2 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 720.0))),
                    safe_area_insets: Some(inset(strip_place(state, None, true, 1.0))),
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                };
                let mut output = ctx.run_ui(raw, |ui| {
                    if let Some(strip) = strip_place(state, None, true, 1.0) {
                        show_strip(ui.ctx(), &p, state, strip);
                    }
                });
                output.textures_delta.clear();
            }
            ctx.is_pointer_over_egui()
        };
        let state = sample();
        assert!(over(&state, egui::pos2(20.0, 15.0)), "a button of the bar");
        assert!(over(&state, egui::pos2(1100.0, 3.0)), "the strip between two buttons");
        assert!(!over(&state, egui::pos2(640.0, 200.0)), "the world under the bar");
        // collapsed: the handle; popped out: nothing of the bar is there
        let collapsed = State {
            collapsed: true,
            ..sample()
        };
        assert!(over(&collapsed, egui::pos2(300.0, 4.0)));
        assert!(!over(&collapsed, egui::pos2(300.0, 15.0)));
        let popped = State { docked: false, ..sample() };
        assert!(!over(&popped, egui::pos2(20.0, 15.0)));
    }

    /// Checkpoint 7's rule, as checkpoint 8 restates it: what the bar's
    /// state holds (the other characters' names, aliases and colours) is
    /// set when the host says so, dropped with the link, and read by the
    /// bar's own drawing: nowhere else in the viewer, so nothing of it
    /// can reach a log, a setting, the chat or a grid. Checked on the
    /// sources, as the interface's rules are (`ui/theme_guard.rs`).
    #[test]
    fn a_viewer_keeps_the_bar_s_state_for_the_bar_and_nothing_else() {
        fn sources(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("the sources").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, into);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    into.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root, &mut files);
        assert!(files.len() > 100, "the viewer's sources are read");
        // (written in two halves: this file is one of the sources)
        let field = ["folderstorm_characters", ".bar"].concat();
        let mut uses = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).expect("a source file");
            let name = path.strip_prefix(&root).expect("under src").to_string_lossy().replace('\\', "/");
            for line in text.lines().map(str::trim) {
                // (`.bar` and what follows: not `.bar_something`)
                let used = line
                    .match_indices(&field)
                    .any(|(at, _)| !line[at + field.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'));
                if used && !line.starts_with("//") {
                    uses.push((name.clone(), line.to_owned()));
                }
            }
        }
        // set by the host's word, and dropped with the link: nothing else
        assert_eq!(uses.len(), 2, "{uses:#?}");
        for (name, line) in &uses {
            assert_eq!(name, "app/folderstorm_characters.rs", "{line}");
            assert!(line.ends_with("= Some(*bar),") || line.ends_with("= None;"), "{line}");
        }
        // its one reader: the bar's drawing, in its strip
        let glue = include_str!("../app/folderstorm_characters/bar.rs");
        assert_eq!(glue.matches("state.bar").count(), 1, "one place reads it");
        assert!(glue.contains("drawn::show_strip(ctx, p, state, strip)") && glue.contains("drawn::inset(strip)"));
        assert!(glue.contains("drawn::strip_place(state, world, bars_in_viewport"));
        assert_eq!(
            glue.matches("folderstorm_bar_state()").count(),
            2,
            "its use: the strip's place (its height and where), the drawing"
        );
        // and no other part of the frame's side reaches it under another name
        for other in [
            include_str!("../app/folderstorm_characters.rs"),
            include_str!("../app/folderstorm_characters/chat.rs"),
            include_str!("../app/folderstorm_characters/monitor.rs"),
        ] {
            assert!(!other.contains("state.bar"), "the bar's state is read outside its drawing");
        }
    }

    #[test]
    fn a_long_name_is_cut_and_whole_in_the_hover() {
        assert_eq!(cut("Main", NAME), "Main");
        assert_eq!(cut("exactly sixteen!", NAME), "exactly sixteen!");
        let short = cut("A very long profile name that goes on", NAME);
        assert_eq!(short.chars().count(), NAME);
        assert!(short.ends_with('…') && short.starts_with("A very long pro"));
        let state = sample();
        let p = crate::theme::Theme::default().skin_palette();
        let quick = quick_faces(&p, &state);
        // the character's button: number and cut name; number alone with
        // icons only; the whole name in the hover
        assert_eq!(quick[2].label, format!("3 · {short}"));
        assert_eq!(quick[2].short, "3");
        assert!(quick[2].tip.contains("A very long profile name that goes on"), "{}", quick[2].tip);
        // behind an alias, the real name is in the hover
        assert!(quick[0].tip.contains("Jane Doe"));
        assert!(full_name(&state.characters[0]).contains("Main (Jane Doe)"));
    }

    #[test]
    fn the_quick_actions_carry_what_they_ask_and_the_characters_their_colour() {
        let state = sample();
        let p = crate::theme::Theme::default().skin_palette();
        let quick = quick_faces(&p, &state);
        let asks: Vec<_> = quick.iter().filter_map(|face| face.ask).collect();
        assert_eq!(
            asks,
            [
                Request::Pick(0),
                Request::Pick(1),
                Request::Pick(2),
                Request::Pick(3),
                Request::Inbox,
                Request::MuteBackground(true),
                Request::VoiceFollows(false),
            ]
        );
        assert!(quick[0].tint.is_some() && quick[1].tint.is_some() && quick[2].tint.is_none());
        // the active character's is the accent's; Attention with
        // something waiting is the caution's, and says how many
        assert_eq!(quick[0].face, ActionKind::Primary);
        assert_eq!(quick[1].face, ActionKind::Flat);
        assert_eq!((quick[4].face, quick[4].short.as_str()), (ActionKind::Caution, "3"));
        assert!(quick[4].label.ends_with("· 3"));
        // the toggles: lit when on
        assert_eq!((quick[5].face, quick[5].icon), (ActionKind::Flat, "volume"));
        assert_eq!(quick[6].face, ActionKind::Primary);
        let muted = State {
            mute_background: true,
            inbox: 0,
            ..sample()
        };
        let quick = quick_faces(&p, &muted);
        assert_eq!((quick[5].face, quick[5].icon), (ActionKind::Primary, "volume-off"));
        assert_eq!(quick[5].ask, Some(Request::MuteBackground(false)));
        assert_eq!((quick[4].face, quick[4].short.as_str()), (ActionKind::Flat, ""));
    }

    #[test]
    fn the_toolbar_says_who_is_in_front_and_which_login_opens() {
        let state = sample();
        let left = left_faces(&state, false);
        assert_eq!(left.len(), 5);
        assert_eq!(left[0].label, "1: Main");
        assert!(left[0].tip.contains("Jane Doe"));
        assert_eq!(left[0].short, "1: Main", "with icons only too");
        assert_eq!(left_faces(&state, true)[0].label, "1");
        // "open a login" is the first character not logged in
        assert_eq!(left[1].ask, Some(Request::OpenLogin(None)));
        assert!(left[1].enabled && left[1].tip.contains("3: A very long profile name"));
        // the chat's button toggles its window and counts what is unread
        assert_eq!(left[2].ask, Some(Request::Chat(true)));
        assert_eq!((left[2].face, left[2].short.as_str()), (ActionKind::Flat, "12"));
        let open = State {
            chat_open: true,
            chat_unread: 0,
            characters: Vec::new(),
            ..sample()
        };
        let left = left_faces(&open, false);
        assert_eq!((left[2].ask, left[2].face), (Some(Request::Chat(false)), ActionKind::Primary));
        assert_eq!(left[0].label, tr!("folderstorm-bar-characters"));
        assert_eq!(left_faces(&open, true)[0].short, "");
        assert!(!left[1].enabled && left[1].ask.is_none());
        assert_eq!(count_text(100), "99+");
    }

    #[test]
    fn the_right_end_is_the_viewer_s_or_the_host_s() {
        let asks = |home| right_faces(home).iter().filter_map(|face| face.ask).collect::<Vec<_>>();
        assert_eq!(asks(Home::Viewer), [Request::Dock(false), Request::Collapse(true)]);
        assert_eq!(asks(Home::Host { pinned: false }), [Request::Pin(true), Request::Dock(true)]);
        assert_eq!(asks(Home::Host { pinned: true }), [Request::Pin(false), Request::Dock(true)]);
        // pinned: another pin, on the accent
        let (plain, pinned) = (right_faces(Home::Host { pinned: false }), right_faces(Home::Host { pinned: true }));
        assert_eq!((plain[0].icon, plain[0].face), ("pin-filled", ActionKind::Flat));
        assert_eq!((pinned[0].icon, pinned[0].face), ("pinned-filled", ActionKind::Primary));
        assert_ne!(plain[0].tip, pinned[0].tip);
    }

    /// Every icon the bar names is one the viewer embeds: an unknown name
    /// draws a button without its icon.
    #[test]
    fn the_bar_s_icons_exist() {
        let p = crate::theme::Theme::default().skin_palette();
        let mut names: Vec<&str> = Vec::new();
        for state in [
            sample(),
            State {
                mute_background: true,
                ..sample()
            },
        ] {
            names.extend(left_faces(&state, false).iter().map(|face| face.icon));
            names.extend(quick_faces(&p, &state).iter().map(|face| face.icon));
        }
        for home in [Home::Viewer, Home::Host { pinned: false }, Home::Host { pinned: true }] {
            names.extend(right_faces(home).iter().map(|face| face.icon));
        }
        for quick in [
            Quick::Switch,
            Quick::Show,
            Quick::LogIn,
            Quick::Wait,
            Quick::Launch,
            Quick::Restart,
            Quick::Nothing,
        ] {
            names.push(quick_face(quick, String::new()).0);
        }
        names.extend(["keyboard-filled", "app-window-filled", "power", "refresh", "x", "users"]);
        for name in names {
            assert!(crate::ui::icons::embedded(name), "{name} is not an embedded icon");
        }
    }
}
