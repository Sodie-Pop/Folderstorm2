//! The window's side of checkpoint 4, "comfort" (session-host plan,
//! section 13): the quick-action strip, the hotkeys' page, "New character
//! profile…", who holds the voice and what is heard in the background.
//! In a file of its own so that the characters' page (`ui.rs`) only calls
//! into it.

use super::{BUTTON, Window_};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use ::folderstorm_session::copy::Source;
use ::folderstorm_session::host::{BackgroundSound, Quick, Slot, Voice};
use ::folderstorm_session::hotkeys::{ACTIONS, Action, Config, ParseError, State};
use ::folderstorm_session::ipc::MAX_SLOTS;
use ::folderstorm_session::profile::{self, ProfileName};
use aurora_i18n::tr;
use egui::RichText;
use std::time::Instant;

use super::super::comfort::{NewProfile, NewProfileError};
use super::super::hotkeys::Registrar;
use crate::folderstorm_characters::profile_copy::{CopyError, Preferences};

/// What the characters' column shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Characters,
    NewProfile,
}

/// What the window keeps for checkpoint 4.
#[derive(Default)]
pub(super) struct Panel {
    pub page: Page,
    /// A hotkey's text while it is typed (None: as the settings hold it).
    keys: [Option<String>; ACTIONS],
    /// "New character profile…": the name typed, what it starts from (0:
    /// the main profile, 1: the defaults, then the profiles of
    /// `profiles`), and why it was not made.
    pub profile_name: String,
    pub profile_from: usize,
    profiles: Vec<ProfileName>,
    profile_error: Option<String>,
    /// What the last profile made is told with, on the characters' page.
    note: Option<String>,
}

/// What a click asks for; carried out after the frame.
pub(super) enum Act {
    Page(Page),
    Quick(usize),
    Sound(usize, BackgroundSound),
    Hotkey(Action, String),
    HotkeysEnabled(bool),
    HotkeysDefaults,
    /// The demo's "Try": what the hotkey would do.
    Try(Action),
    CreateProfile,
}

/// The longest name a quick-action button shows.
const QUICK_NAME: usize = 16;

/// What a quick action's button shows, and what it says it will do.
pub(super) fn quick_face(quick: Quick, name: String) -> (&'static str, String) {
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

/// What an action is called.
fn action_name(action: Action) -> String {
    match action {
        Action::Character(slot) => tr!("folderstorm-hotkeys-character", number = slot + 1),
        Action::Next => tr!("folderstorm-hotkeys-next").to_owned(),
        Action::Previous => tr!("folderstorm-hotkeys-previous").to_owned(),
    }
}

/// A hotkey's state in words, and whether it is a problem.
fn state_text(state: &State) -> Option<(String, bool)> {
    let text = match state {
        State::Unset => return None,
        State::Disabled => tr!("folderstorm-hotkeys-off").to_owned(),
        State::Registered => tr!("folderstorm-hotkeys-registered").to_owned(),
        State::Unavailable => tr!("folderstorm-hotkeys-unavailable").to_owned(),
        State::Taken => tr!("folderstorm-hotkeys-taken").to_owned(),
        State::TypesCharacter => tr!("folderstorm-hotkeys-types-character").to_owned(),
        State::Duplicate(first) => tr!("folderstorm-hotkeys-duplicate", other = action_name(*first)),
        State::Invalid(ParseError::NoKey) => tr!("folderstorm-hotkeys-no-key").to_owned(),
        State::Invalid(ParseError::TwoKeys) => tr!("folderstorm-hotkeys-two-keys").to_owned(),
        State::Invalid(ParseError::Unknown(word)) => tr!("folderstorm-hotkeys-unknown", word = word.clone()),
        State::Invalid(ParseError::NoModifier) => tr!("folderstorm-hotkeys-no-modifier").to_owned(),
        State::Invalid(ParseError::OneModifier) => tr!("folderstorm-hotkeys-one-modifier").to_owned(),
        State::Invalid(ParseError::Reserved) => tr!("folderstorm-hotkeys-reserved").to_owned(),
    };
    Some((text, state.problem()))
}

/// Why a profile was not made, for the user.
fn new_profile_error(name: &str, error: NewProfileError) -> String {
    let name = name.trim().to_owned();
    match error {
        NewProfileError::Name(error) => super::add_error(&name, ::folderstorm_session::host::AddError::Name(error)),
        NewProfileError::Copy(CopyError::Running) => tr!("folderstorm-new-profile-running", name = name),
        NewProfileError::Copy(CopyError::Exists) => tr!("folderstorm-new-profile-exists", name = name),
        NewProfileError::Copy(CopyError::SourceMissing) => tr!("folderstorm-new-profile-source-missing").to_owned(),
        NewProfileError::Copy(CopyError::Io(_)) => tr!("folderstorm-new-profile-failed").to_owned(),
    }
}

impl Window_ {
    /// The page shown in place of the characters, if any: true when one
    /// was drawn.
    pub(super) fn comfort_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<super::Act>) -> bool {
        let p = self.palette;
        let mut asked = Vec::new();
        match self.comfort.page {
            Page::Characters => return false,
            Page::NewProfile => self.new_profile_page(ui, &p, &mut asked),
        }
        acts.extend(asked.into_iter().map(super::Act::Comfort));
        true
    }

    /// One button per character that does the right thing for where its
    /// viewer is: the quick actions, which the hotkeys press too.
    pub(super) fn quick_strip(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<super::Act>) {
        let desk = &self.host.desk;
        if desk.slots().next().is_none() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (slot, state) in desk.slots() {
                let quick = desk.quick(slot);
                let name = super::shown_name(state);
                let short: String = name.chars().take(QUICK_NAME).collect();
                let (icon, mut help) = quick_face(quick, name);
                if self.host.hotkey_state(Action::Character(slot)) == State::Registered {
                    help.push('\n');
                    let keys = desk.hotkeys().text(Action::Character(slot)).to_owned();
                    help.push_str(&tr!("folderstorm-quick-hotkey", keys = keys));
                }
                if state.alias.is_some() {
                    // (checkpoint 7: the name behind the alias)
                    help.push('\n');
                    help.push_str(&super::super::faces::real_name(state));
                }
                let kind = if desk.active() == Some(slot) {
                    ActionKind::Primary
                } else {
                    ActionKind::Flat
                };
                let label = format!("{} · {short}", slot + 1);
                let enabled = quick != Quick::Nothing;
                let button = widgets::action_button_if(ui, p, icon, &label, kind, BUTTON, enabled);
                // (checkpoint 7: the character's colour under its button)
                if let Some(tint) = super::super::faces::tint(p, state.colour) {
                    super::super::faces::underline(ui, button.rect, tint);
                }
                if button.on_hover_text(help).clicked() && enabled {
                    acts.push(super::Act::Comfort(Act::Quick(slot)));
                }
            }
        });
        ui.add_space(8.0);
    }

    /// Under the characters: the ways to the two other pages, what the
    /// last profile made is told with, and the hotkeys that do not work.
    pub(super) fn comfort_buttons(&self, ui: &mut egui::Ui, p: &Palette, acts: &mut Vec<super::Act>) {
        ui.horizontal_wrapped(|ui| {
            let new = tr!("folderstorm-host-new-profile");
            if widgets::action_button(ui, p, "user-plus", new, ActionKind::Flat, BUTTON)
                .on_hover_text(tr!("folderstorm-host-new-profile-help"))
                .clicked()
            {
                acts.push(super::Act::Comfort(Act::Page(Page::NewProfile)));
            }
            // (checkpoint 10: the hotkeys, the bar's and the monitors'
            // pages are sections of the settings window)
            let settings = tr!("folderstorm-settings-open");
            if widgets::action_button(ui, p, "settings-filled", settings, ActionKind::Flat, BUTTON)
                .on_hover_text(tr!("folderstorm-settings-open-help"))
                .clicked()
            {
                let open = super::settings::Act::Open(::folderstorm_session::settings::Section::Toolbar, None);
                acts.push(super::Act::Settings(open));
            }
        });
        if let Some(note) = &self.comfort.note {
            ui.label(RichText::new(note).color(p.ink));
        }
        // a hotkey that was refused is said here, not only on its page
        let problems = self.host.hotkey_problems();
        if problems > 0 {
            ui.label(RichText::new(tr!("folderstorm-hotkeys-problems", count = problems)).color(p.amber));
        }
    }

    /// Beside a character's name: what it holds of the voice.
    pub(super) fn voice_badge(&self, ui: &mut egui::Ui, p: &Palette, slot: usize) {
        match self.host.desk.voice(slot) {
            Voice::Allowed => {
                ui.label(RichText::new(tr!("folderstorm-host-voice-here")).strong().color(p.violet_light))
                    .on_hover_text(tr!("folderstorm-host-voice-here-help"));
            }
            Voice::GivingUp => {
                ui.label(RichText::new(tr!("folderstorm-host-voice-giving-up")).color(p.amber));
            }
            // (said under the list, with the name of the one waited for)
            Voice::WaitsFor(_) | Voice::Off => {}
        }
    }

    /// In a character's row: heard while another one is active, or not.
    pub(super) fn sound_choice(&self, ui: &mut egui::Ui, p: &Palette, slot: usize, state: &Slot, acts: &mut Vec<super::Act>) {
        let mut heard = state.sound == BackgroundSound::All;
        if widgets::checkbox(ui, p, &mut heard, tr!("folderstorm-host-sound-all"))
            .on_hover_text(tr!("folderstorm-host-sound-all-help"))
            .changed()
        {
            let sound = if heard { BackgroundSound::All } else { BackgroundSound::Muted };
            acts.push(super::Act::Comfort(Act::Sound(slot, sound)));
        }
    }

    fn page_heading(&self, ui: &mut egui::Ui, p: &Palette, heading: &str, intro: &str, asked: &mut Vec<Act>) {
        if widgets::action_button(ui, p, "arrow-left", tr!("common-back"), ActionKind::Flat, BUTTON).clicked() {
            asked.push(Act::Page(Page::Characters));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(heading).size(18.0).strong().color(p.ink));
        ui.add_space(2.0);
        ui.label(RichText::new(intro).color(p.muted));
        ui.add_space(10.0);
    }

    /// Checkpoint 10: the hotkeys' section of the settings window (it was
    /// checkpoint 4's page).
    pub(super) fn hotkeys_settings(&mut self, ui: &mut egui::Ui, p: &Palette, asked: &mut Vec<Act>) {
        if !Registrar::available() {
            ui.label(RichText::new(tr!("folderstorm-hotkeys-windows-only")).color(p.amber));
            ui.add_space(6.0);
        }
        let config = self.host.desk.hotkeys().clone();
        let mut enabled = config.enabled;
        if widgets::checkbox(ui, p, &mut enabled, tr!("folderstorm-hotkeys-enabled")).changed() {
            asked.push(Act::HotkeysEnabled(enabled));
        }
        ui.add_space(8.0);
        let demo = self.host.demo;
        // (in the settings' page, which scrolls)
        ui.vertical(|ui| {
            for action in Action::all() {
                let index = action.index();
                let state = self.host.hotkey_state(action);
                ui.horizontal(|ui| {
                    let name = RichText::new(action_name(action)).color(p.ink);
                    ui.add_sized([150.0, BUTTON.y], egui::Label::new(name).truncate());
                    let mut text = self.comfort.keys[index].clone().unwrap_or_else(|| config.text(action).to_owned());
                    let field = widgets::text_field(ui, true, &mut text, |b, _| {
                        egui::TextEdit::singleline(b)
                            .char_limit(::folderstorm_session::hotkeys::MAX_TEXT)
                            .hint_text(tr!("folderstorm-hotkeys-hint"))
                            .desired_width(170.0)
                    });
                    if field.changed() {
                        self.comfort.keys[index] = Some(text);
                    }
                    if field.lost_focus()
                        && let Some(typed) = self.comfort.keys[index].take()
                    {
                        asked.push(Act::Hotkey(action, typed));
                    }
                    if demo
                        && widgets::action_button(
                            ui,
                            p,
                            "player-play-filled",
                            tr!("folderstorm-hotkeys-try"),
                            ActionKind::Flat,
                            BUTTON,
                        )
                        .on_hover_text(tr!("folderstorm-hotkeys-try-help"))
                        .clicked()
                    {
                        asked.push(Act::Try(action));
                    }
                });
                if let Some((text, problem)) = state_text(&state) {
                    let colour = if problem { p.danger } else { p.muted };
                    ui.label(RichText::new(text).color(colour));
                }
                ui.add_space(6.0);
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(tr!("folderstorm-hotkeys-altgr-note")).color(p.muted));
        ui.add_space(6.0);
        if widgets::action_button(ui, p, "refresh", tr!("folderstorm-hotkeys-defaults"), ActionKind::Flat, BUTTON).clicked() {
            asked.push(Act::HotkeysDefaults);
        }
    }

    fn new_profile_page(&mut self, ui: &mut egui::Ui, p: &Palette, asked: &mut Vec<Act>) {
        self.page_heading(
            ui,
            p,
            tr!("folderstorm-new-profile-heading"),
            tr!("folderstorm-new-profile-intro"),
            asked,
        );
        let field = widgets::text_field(ui, true, &mut self.comfort.profile_name, |b, ui| {
            egui::TextEdit::singleline(b)
                .char_limit(profile::MAX_LEN)
                .hint_text(tr!("folderstorm-host-add-hint"))
                .desired_width((ui.available_width() - 16.0).clamp(120.0, 320.0))
        });
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(10.0);
        ui.label(RichText::new(tr!("folderstorm-new-profile-from")).strong().color(p.ink));
        ui.add_space(4.0);
        let from = &mut self.comfort.profile_from;
        if widgets::radio(ui, p, *from == 0, tr!("folderstorm-new-profile-from-main")).clicked() {
            *from = 0;
        }
        if widgets::radio(ui, p, *from == 1, tr!("folderstorm-new-profile-from-defaults")).clicked() {
            *from = 1;
        }
        if !self.comfort.profiles.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("folderstorm-host-new-profile-from")
                .auto_shrink([false, true])
                .max_height((ui.available_height() - 110.0).max(60.0))
                .show(ui, |ui| {
                    for (i, name) in self.comfort.profiles.iter().enumerate() {
                        let label = tr!("folderstorm-new-profile-from-profile", name = name.shown().to_owned());
                        if widgets::radio(ui, p, *from == i + 2, label).clicked() {
                            *from = i + 2;
                        }
                    }
                });
        }
        ui.add_space(10.0);
        if let Some(error) = &self.comfort.profile_error {
            ui.label(RichText::new(error).color(p.danger));
            ui.add_space(4.0);
        }
        ui.horizontal_wrapped(|ui| {
            let create = tr!("folderstorm-new-profile-create");
            if widgets::action_button(ui, p, "user-plus", create, ActionKind::Primary, BUTTON).clicked() || entered {
                asked.push(Act::CreateProfile);
            }
            if widgets::action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked() {
                asked.push(Act::Page(Page::Characters));
            }
        });
    }

    /// What the new profile starts from, as chosen on its page.
    fn profile_source(&self) -> Source {
        match self.comfort.profile_from {
            0 => Source::Main,
            1 => Source::Defaults,
            i => match self.comfort.profiles.get(i - 2) {
                Some(name) => Source::Profile(name.clone()),
                None => Source::Main,
            },
        }
    }

    pub(super) fn comfort_apply(&mut self, act: Act, now: Instant) {
        match act {
            Act::Page(page) => {
                if page == Page::NewProfile {
                    self.comfort.profiles = self.host.profiles();
                    self.comfort.profile_error = None;
                    if self.comfort.profile_from >= self.comfort.profiles.len() + 2 {
                        self.comfort.profile_from = 0;
                    }
                }
                if page != Page::Characters {
                    self.comfort.note = None;
                }
                self.comfort.keys = Default::default();
                self.comfort.page = page;
            }
            Act::Quick(slot) => {
                let quick = self.host.desk.quick_act(slot, now);
                log::info!("session host: quick action for character {}: {quick:?}", slot + 1);
            }
            Act::Sound(slot, sound) => self.host.desk.set_sound(slot, sound),
            Act::Hotkey(action, text) => {
                if self.host.desk.set_hotkey(action, &text) {
                    self.host.apply_hotkeys();
                }
            }
            Act::HotkeysEnabled(enabled) => {
                let config = Config {
                    enabled,
                    ..self.host.desk.hotkeys().clone()
                };
                if self.host.desk.set_hotkeys(config) {
                    self.host.apply_hotkeys();
                }
            }
            Act::HotkeysDefaults => {
                let config = Config {
                    enabled: self.host.desk.hotkeys().enabled,
                    ..Config::default()
                };
                self.comfort.keys = Default::default();
                if self.host.desk.set_hotkeys(config) {
                    self.host.apply_hotkeys();
                }
            }
            Act::Try(action) => self.host.hotkey(action, now),
            Act::CreateProfile => {
                let source = self.profile_source();
                match self.host.new_profile(&self.comfort.profile_name, &source) {
                    Ok(NewProfile { name, created, listed }) => {
                        let shown = name.shown().to_owned();
                        let mut note = if listed {
                            tr!("folderstorm-new-profile-done", name = shown)
                        } else {
                            tr!("folderstorm-new-profile-done-full", name = shown, max = MAX_SLOTS)
                        };
                        if created.preferences == Preferences::Unreadable {
                            note.push(' ');
                            note.push_str(tr!("folderstorm-new-profile-done-unreadable"));
                        }
                        self.comfort.note = Some(note);
                        self.comfort.profile_name.clear();
                        self.comfort.profile_error = None;
                        self.comfort.page = Page::Characters;
                    }
                    Err(error) => {
                        self.comfort.profile_error = Some(new_profile_error(&self.comfort.profile_name, error));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_ui::tests::host;
    use super::super::tests::{draw, draw_in, window};
    use super::*;
    use ::folderstorm_session::host::Effect;

    fn shows(drawn: &[String], text: &str) -> bool {
        drawn.iter().any(|t| t.contains(text))
    }

    #[test]
    fn the_characters_page_has_the_quick_actions_and_the_ways_to_the_other_pages() {
        let mut app = window(host());
        draw(&mut app, 1120, 760);
        let (drawn, _) = draw(&mut app, 1120, 760);
        // one quick-action button per character, numbered as its hotkey
        assert!(shows(&drawn, "1 · demo-1") && shows(&drawn, "2 · demo-2"), "{drawn:?}");
        assert!(shows(&drawn, "New character profile…") && shows(&drawn, tr!("folderstorm-settings-open")));
        assert!(shows(&drawn, "Heard in the background"));
        // the quick action of a character without a viewer starts one, once
        let now = Instant::now();
        app.comfort_apply(Act::Quick(0), now);
        app.comfort_apply(Act::Quick(0), now);
        let spawns = app
            .host
            .desk
            .take_effects()
            .into_iter()
            .filter(|e| matches!(e, Effect::Spawn { .. }))
            .count();
        assert_eq!(spawns, 1);
        // heard in the background: the desk's choice
        app.comfort_apply(Act::Sound(1, BackgroundSound::All), now);
        assert_eq!(app.host.desk.slot(1).expect("slot").sound, BackgroundSound::All);
    }

    #[test]
    fn the_hotkeys_page_shows_each_hotkey_and_what_is_wrong_with_it() {
        let mut app = window(host());
        let now = Instant::now();
        app.host.apply_hotkeys();
        // (checkpoint 10: a section of the settings window)
        let open = super::super::settings::Act::Open(::folderstorm_session::settings::Section::Hotkeys, None);
        app.apply(vec![super::super::Act::Settings(open)], now);
        app.settings.view = Some(app.look.view());
        let settings = super::super::panels::Which::Settings;
        draw_in(&mut app, settings, 1000, 900);
        let (drawn, _) = draw_in(&mut app, settings, 1000, 900);
        assert!(shows(&drawn, tr!("folderstorm-settings-hotkeys")), "{drawn:?}");
        assert!(shows(&drawn, "Character 1") && shows(&drawn, "Next character") && shows(&drawn, "Previous character"));
        assert!(shows(&drawn, "Ctrl+Shift+F1") && shows(&drawn, "Ctrl+Shift+F7"));
        // (the tests register nothing: as on a system without them)
        assert!(shows(&drawn, "Windows only"));
        // a choice that is no hotkey, and one already used: both are told
        app.comfort_apply(Act::Hotkey(Action::Next, "ctrl+banana".into()), now);
        app.comfort_apply(Act::Hotkey(Action::Previous, "ctrl + shift + f2".into()), now);
        let (drawn, _) = draw_in(&mut app, settings, 1000, 900);
        assert!(shows(&drawn, "banana"), "{drawn:?}");
        assert!(shows(&drawn, "Same keys as") && shows(&drawn, "which keeps them"));
        assert_eq!(app.host.desk.hotkeys().text(Action::Previous), "Ctrl+Shift+F2");
        assert_eq!(app.host.hotkey_problems(), 2);
        // and on the characters' page
        app.comfort_apply(Act::Page(Page::Characters), now);
        let (drawn, _) = draw(&mut app, 1120, 900);
        assert!(shows(&drawn, "hotkeys do not work"), "{drawn:?}");
        // back to the defaults; off keeps the choices
        app.comfort_apply(Act::HotkeysDefaults, now);
        assert_eq!(app.host.hotkey_problems(), 0);
        app.comfort_apply(Act::HotkeysEnabled(false), now);
        assert!(!app.host.desk.hotkeys().enabled);
        assert_eq!(app.host.desk.hotkeys().text(Action::Character(0)), "Ctrl+Shift+F1");
        // the demo's "Try" does what the hotkey would
        app.host.desk.take_effects();
        app.comfort_apply(Act::Try(Action::Character(1)), now);
        assert!(app.host.desk.take_effects().contains(&Effect::Spawn { slot: 1, launch: 1 }));
    }

    #[test]
    fn a_new_profile_is_made_from_the_page_and_joins_the_list() {
        let mut app = window(host());
        let now = Instant::now();
        // (the tests' settings folder is shared by the test run: a name of
        // this test's own)
        let name = format!("np-{}", crate::folderstorm_characters::transport::random_hex(8));
        app.comfort_apply(Act::Page(Page::NewProfile), now);
        draw(&mut app, 1120, 760);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(
            shows(&drawn, "New character profile") && shows(&drawn, "The main profile"),
            "{drawn:?}"
        );
        assert!(shows(&drawn, "Create profile"));
        // no name: told, nothing made
        app.comfort_apply(Act::CreateProfile, now);
        assert_eq!(app.comfort.page, Page::NewProfile);
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, "Type a profile name first"), "{drawn:?}");
        // a name that is none
        app.comfort.profile_name = "../x".into();
        app.comfort_apply(Act::CreateProfile, now);
        assert!(app.comfort.profile_error.as_deref().is_some_and(|e| e.contains("not allowed")));
        // from the defaults: made, listed, and the page closes
        app.comfort.profile_name = name.clone();
        app.comfort.profile_from = 1;
        app.comfort_apply(Act::CreateProfile, now);
        assert_eq!(app.comfort.page, Page::Characters);
        assert!(app.host.desk.slots().any(|(_, s)| s.profile.key() == name));
        let (drawn, _) = draw(&mut app, 1120, 760);
        assert!(shows(&drawn, &format!("Profile \"{name}\" made")), "{drawn:?}");
        let dir = crate::settings::config_dir().join("profiles").join(&name);
        assert!(dir.is_dir());
        // the same name again: it exists now
        app.comfort_apply(Act::Page(Page::NewProfile), now);
        assert!(app.comfort.profiles.iter().any(|p| p.key() == name), "offered as a source");
        app.comfort.profile_name = name.to_uppercase();
        app.comfort_apply(Act::CreateProfile, now);
        assert_eq!(app.comfort.page, Page::NewProfile);
        assert!(app.comfort.profile_error.as_deref().is_some_and(|e| e.contains("already exists")));
        // while the host starts that character's viewer: refused as running
        let slot = app
            .host
            .desk
            .slots()
            .find(|(_, s)| s.profile.key() == name)
            .map(|(i, _)| i)
            .expect("slot");
        app.host.desk.launch(slot, false);
        app.comfort_apply(Act::CreateProfile, now);
        assert!(app.comfort.profile_error.as_deref().is_some_and(|e| e.contains("open in a viewer")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
