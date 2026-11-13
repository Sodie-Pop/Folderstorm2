//! Checkpoint 8 of the session host, the docked bar (session-host plan,
//! section 15): one row at the top of every managed viewer's window, fed
//! by the host, from which the characters are driven; popped out, the same
//! row in a compact window of the host's own.
//!
//! The rules, all pure:
//! - what the bar is made of and what the user chose for it ([`Config`]):
//!   docked or popped out, collapsed, pinned, up to [`MAX_QUICK`] quick
//!   actions, how the buttons are labelled; checked when the settings file
//!   is read ([`Config::cleaned`]);
//! - what the host tells a viewer to draw ([`State`]), whole, and what a
//!   click asks of the host ([`Request`]); both are bounded and cleaned on
//!   arrival;
//! - which quick actions show ([`State::buttons`]) and which character
//!   "open a login" means ([`State::next_login`]);
//! - when the host's own window hides ([`host_hidden`]): only while a bar
//!   is on screen to bring it back;
//! - what a viewer's window is on the taskbar and in the Alt-Tab list
//!   ([`taskbar`]): everything is as usual unless the host asks otherwise
//!   of a background character, and a window that is or will be minimised
//!   never becomes a tool window.
//!
//! A viewer only draws the state and sends requests. The host does what
//! the same click does in its own window, nothing more: nothing is
//! answered from the bar, no line is said, no process is ended.
//!
//! **Aliases and colours** (checkpoint 7) are in [`State`]: they reach the
//! viewers this host manages, for the bar and nothing else. A viewer keeps
//! them in the bar's state only, and drops it with the link.

use crate::host::Quick;
use crate::identity::Colour;
use crate::ipc::{MAX_NAME, MAX_SLOTS};
use crate::panels::Place;
use serde::{Deserialize, Serialize};

/// How many positions the quick actions have.
pub const MAX_QUICK: usize = 12;

/// The bar's height in a viewer's window, and the height of the handle a
/// collapsed bar leaves, in the interface's points.
pub const HEIGHT: f32 = 30.0;
pub const HANDLE: f32 = 8.0;

/// The size of the popped-out window's inside the first time it opens,
/// and the smallest it takes, in the screen's points: one row.
pub const WINDOW_SIZE: [u32; 2] = [1100, 38];
pub const WINDOW_MIN_SIZE: [u32; 2] = [320, 38];

/// What a button shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabelMode {
    IconOnly,
    TextOnly,
    /// Also what a name this build does not know reads as.
    #[default]
    #[serde(other)]
    IconAndText,
}

impl LabelMode {
    pub const ALL: [LabelMode; 3] = [LabelMode::IconOnly, LabelMode::TextOnly, LabelMode::IconAndText];

    pub fn index(self) -> usize {
        match self {
            LabelMode::IconOnly => 0,
            LabelMode::TextOnly => 1,
            LabelMode::IconAndText => 2,
        }
    }

    pub fn icon(self) -> bool {
        self != LabelMode::TextOnly
    }

    pub fn text(self) -> bool {
        self != LabelMode::IconOnly
    }

    /// The mode a row that has no room falls back to: its icons alone.
    /// A row of texts only keeps its texts (it has nothing else).
    pub fn narrow(self) -> Self {
        match self {
            LabelMode::IconAndText => LabelMode::IconOnly,
            other => other,
        }
    }
}

/// One position of the quick actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum QuickAction {
    /// Nothing there. Also what a name this build does not know reads as.
    #[default]
    Unused,
    /// A character's quick action (slot 0 to [`MAX_SLOTS`] - 1): switch,
    /// bring forward, start.
    Character(u8),
    /// How many questions wait in the inbox; opens the inbox.
    Attention,
    /// Toggles "Mute background sound".
    MuteBackground,
    /// Toggles "Voice follows active".
    VoiceFollows,
}

impl QuickAction {
    /// Every action a position can hold, in the order they are offered.
    pub fn all() -> impl Iterator<Item = QuickAction> {
        std::iter::once(QuickAction::Unused)
            .chain((0..MAX_SLOTS as u8).map(QuickAction::Character))
            .chain([QuickAction::Attention, QuickAction::MuteBackground, QuickAction::VoiceFollows])
    }

    /// The action's name in the settings file.
    pub fn key(self) -> String {
        match self {
            QuickAction::Unused => "unused".to_owned(),
            QuickAction::Character(slot) => format!("character-{}", u32::from(slot) + 1),
            QuickAction::Attention => "attention".to_owned(),
            QuickAction::MuteBackground => "mute-background".to_owned(),
            QuickAction::VoiceFollows => "voice-follows".to_owned(),
        }
    }

    pub fn parse(key: &str) -> Self {
        match key.trim() {
            "attention" => QuickAction::Attention,
            "mute-background" => QuickAction::MuteBackground,
            "voice-follows" => QuickAction::VoiceFollows,
            other => other
                .strip_prefix("character-")
                .and_then(|number| number.parse::<usize>().ok())
                .filter(|number| (1..=MAX_SLOTS).contains(number))
                .map_or(QuickAction::Unused, |number| QuickAction::Character((number - 1) as u8)),
        }
    }
}

impl From<String> for QuickAction {
    fn from(key: String) -> Self {
        Self::parse(&key)
    }
}

impl From<QuickAction> for String {
    fn from(action: QuickAction) -> Self {
        action.key()
    }
}

/// Where the bar is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// In the host's own compact window.
    Popped,
    /// At the top of every managed viewer's window. Also what a name
    /// this build does not know reads as.
    #[default]
    #[serde(other)]
    Docked,
}

/// The bar in the host's settings file (`session-host.json`, `bar`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub placement: Placement,
    /// The docked bar is a thin handle.
    pub collapsed: bool,
    /// The popped-out window stays over the others.
    pub pinned: bool,
    /// Where the popped-out window was last.
    pub place: Option<Place>,
    /// The quick actions, [`MAX_QUICK`] positions.
    pub quick: Vec<QuickAction>,
    /// How the quick actions are labelled, and the toolbar's own buttons.
    pub quick_labels: LabelMode,
    pub toolbar_labels: LabelMode,
    /// Windows: while docked, only the active character's window is on
    /// the taskbar and in Alt-Tab.
    pub one_taskbar_button: bool,
    /// The host's own window hides while a bar is on screen.
    pub hide_host: bool,
    /// In a viewer with a custom world viewport, the docked bar spans
    /// the whole window; off (the default), it stays over the world
    /// viewport. Without a custom viewport it spans the window either way.
    pub span_window: bool,
}

impl Default for Config {
    fn default() -> Self {
        // the five characters, then what waits for an answer
        let mut quick: Vec<QuickAction> = (0..MAX_SLOTS as u8).map(QuickAction::Character).collect();
        quick.push(QuickAction::Attention);
        quick.resize(MAX_QUICK, QuickAction::Unused);
        Self {
            placement: Placement::Docked,
            collapsed: false,
            pinned: false,
            place: None,
            quick,
            quick_labels: LabelMode::IconAndText,
            toolbar_labels: LabelMode::IconOnly,
            one_taskbar_button: true,
            hide_host: true,
            span_window: false,
        }
    }
}

impl Config {
    /// The file's bar as the host can use it: exactly [`MAX_QUICK`]
    /// positions, each action once (a second position with the same
    /// action is unused), a place a window can have had.
    pub fn cleaned(mut self) -> Self {
        self.quick.truncate(MAX_QUICK);
        self.quick.resize(MAX_QUICK, QuickAction::Unused);
        for position in 0..self.quick.len() {
            let action = self.quick[position];
            if action != QuickAction::Unused && self.quick[..position].contains(&action) {
                self.quick[position] = QuickAction::Unused;
            }
        }
        self.place = self.place.filter(place_valid);
        self
    }

    /// Sets one position; the action leaves the position it had before
    /// (each action shows once). False when nothing changed.
    pub fn set_quick(&mut self, position: usize, action: QuickAction) -> bool {
        if position >= MAX_QUICK || self.quick.get(position) == Some(&action) {
            return false;
        }
        self.quick.resize(MAX_QUICK, QuickAction::Unused);
        if action != QuickAction::Unused {
            for held in &mut self.quick {
                if *held == action {
                    *held = QuickAction::Unused;
                }
            }
        }
        self.quick[position] = action;
        true
    }

    /// The popped-out window was moved or resized; false when it is
    /// where it was, or no place for a window.
    pub fn placed(&mut self, place: Place) -> bool {
        if !place_valid(&place) || self.place == Some(place) {
            return false;
        }
        self.place = Some(place);
        true
    }
}

/// A place the compact window can have had: one row is far lower than a
/// panel's least window, so only the numbers are checked.
fn place_valid(place: &Place) -> bool {
    const FAR: i32 = 1 << 16;
    (100..=16_384).contains(&place.size[0])
        && (16..=16_384).contains(&place.size[1])
        && place.position.is_none_or(|at| at.iter().all(|c| (-FAR..=FAR).contains(c)))
}

/// Where a character is, for the bar's list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Presence {
    /// In the world and the active character.
    Active,
    /// In the world, behind the active one.
    Inactive,
    /// Not logged in: no viewer, one that starts, or its login screen.
    #[default]
    Out,
}

/// One character as the bar shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    pub slot: u8,
    /// Its alias when it has one, else its name.
    pub name: String,
    /// Its real name, when the alias stands in its place (the hover).
    pub real: Option<String>,
    pub colour: Colour,
    pub presence: Presence,
    /// What picking it does now (section 13's quick action).
    pub quick: Quick,
    /// Its monitor is open.
    pub monitor: bool,
}

impl Character {
    /// Its monitor can be opened or closed from the bar: it is in the
    /// world and not the one in front.
    pub fn watchable(&self) -> bool {
        self.presence == Presence::Inactive
    }
}

/// The bar as the host wants it drawn, whole: the latest one replaces
/// the others.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The viewers draw it. False: the host's own window has it, and a
    /// viewer draws nothing and keeps no strip.
    pub docked: bool,
    pub collapsed: bool,
    /// The characters of the desk, by slot.
    pub characters: Vec<Character>,
    /// How many questions wait in the inbox.
    pub inbox: u32,
    /// The shared chat has its window; its unread lines.
    pub chat_open: bool,
    pub chat_unread: u32,
    pub mute_background: bool,
    pub voice_follows: bool,
    pub quick: Vec<QuickAction>,
    pub quick_labels: LabelMode,
    pub toolbar_labels: LabelMode,
    /// [`Config::span_window`]. (Absent, as from a host before it: over
    /// the world viewport. Host and viewers are one binary, so no new
    /// protocol number.)
    #[serde(default)]
    pub span_window: bool,
}

/// A quick action as the bar shows it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button<'a> {
    Character(&'a Character),
    /// With how many wait: shown even when none does.
    Attention(u32),
    /// With whether it is on.
    MuteBackground(bool),
    VoiceFollows(bool),
}

impl State {
    /// The state with every text bounded and without control characters,
    /// at most [`MAX_SLOTS`] characters (one per slot) and [`MAX_QUICK`]
    /// quick actions: what a viewer keeps and draws of what came down
    /// its pipe.
    pub fn cleaned(mut self) -> Self {
        let mut seen = [false; MAX_SLOTS];
        self.characters.retain_mut(|c| {
            let Some(first) = seen.get_mut(usize::from(c.slot)).filter(|seen| !**seen) else {
                return false;
            };
            *first = true;
            c.name = clean(&c.name);
            c.real = c.real.take().map(|real| clean(&real)).filter(|real| !real.is_empty());
            true
        });
        self.characters.sort_by_key(|c| c.slot);
        self.quick.truncate(MAX_QUICK);
        self
    }

    pub fn character(&self, slot: u8) -> Option<&Character> {
        self.characters.iter().find(|c| c.slot == slot)
    }

    /// The character in front, if it is in the world.
    pub fn active(&self) -> Option<&Character> {
        self.characters.iter().find(|c| c.presence == Presence::Active)
    }

    /// The quick actions that show, in their positions' order: unused
    /// positions and characters the desk does not have are left out;
    /// Attention shows whatever its count.
    pub fn buttons(&self) -> Vec<Button<'_>> {
        self.quick
            .iter()
            .filter_map(|action| match *action {
                QuickAction::Unused => None,
                QuickAction::Character(slot) => self.character(slot).map(Button::Character),
                QuickAction::Attention => Some(Button::Attention(self.inbox)),
                QuickAction::MuteBackground => Some(Button::MuteBackground(self.mute_background)),
                QuickAction::VoiceFollows => Some(Button::VoiceFollows(self.voice_follows)),
            })
            .collect()
    }

    /// The characters "open a login" can open: not logged in, and with
    /// something to do for them (start a viewer, or bring one forward at
    /// its login screen).
    pub fn logins(&self) -> impl Iterator<Item = &Character> {
        self.characters
            .iter()
            .filter(|c| c.presence == Presence::Out && !matches!(c.quick, Quick::Nothing | Quick::Switch | Quick::Show))
    }

    /// The character "open a login" means: the first one not logged in.
    pub fn next_login(&self) -> Option<&Character> {
        self.logins().next()
    }

    /// The strip a viewer keeps at the top of its window for the bar, in
    /// the interface's points: none when the bar is not docked.
    pub fn strip(&self) -> f32 {
        match (self.docked, self.collapsed) {
            (false, _) => 0.0,
            (true, false) => HEIGHT,
            (true, true) => HANDLE,
        }
    }
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// A page of the host's own window the bar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    /// The bar's and the quick actions' settings.
    Bar,
    Hotkeys,
    /// The characters, where each one's monitor is set.
    Monitors,
    /// The window as it is.
    Host,
    /// The question that closes the host or every character.
    Quit,
}

/// One click on the bar, for the host to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// A character was picked (in the list, or by its quick action):
    /// switch to it, bring it forward at its login screen, or start it.
    Pick(u8),
    /// Open a login: that character's, or the first one not logged in.
    OpenLogin(Option<u8>),
    /// The shared chat's window, shown or hidden.
    Chat(bool),
    /// A character's monitor, opened or closed.
    Monitor(u8, bool),
    /// A monitor for every character that is in the world behind the
    /// active one; no monitor at all.
    OpenMonitors,
    CloseMonitors,
    /// The host's own window, on that page.
    Show(Page),
    /// The inbox, in its own window.
    Inbox,
    MuteBackground(bool),
    VoiceFollows(bool),
    /// The bar leaves the viewers for the host's compact window, or
    /// comes back.
    Dock(bool),
    Collapse(bool),
    /// The popped-out window stays over the others (asked there only).
    Pin(bool),
}

impl Request {
    /// A request the host may act on: it names a slot there can be.
    pub fn valid(&self) -> bool {
        match *self {
            Request::Pick(slot) | Request::OpenLogin(Some(slot)) | Request::Monitor(slot, _) => usize::from(slot) < MAX_SLOTS,
            _ => true,
        }
    }

    /// The click opens or shows a window of the host's: the viewer that
    /// was clicked lets the host come to the front (Windows).
    pub fn shows_a_window(&self) -> bool {
        matches!(
            self,
            Request::Pick(_)
                | Request::OpenLogin(_)
                | Request::Chat(true)
                | Request::Monitor(_, true)
                | Request::OpenMonitors
                | Request::Show(_)
                | Request::Inbox
                | Request::Dock(false)
        )
    }
}

/// Whether the host's own window is hidden: only by the user's choice
/// (`hide`), only while a bar is on screen to bring it back, and not
/// while the user asked to see it (`asked`). `bars`: the viewers that
/// draw the docked bar now; `characters`: how many the desk has (a
/// compact window without a character has nothing to drive: the host's
/// window stays, to add one).
pub fn host_hidden(hide: bool, asked: bool, placement: Placement, bars: usize, characters: usize) -> bool {
    hide && !asked
        && match placement {
            Placement::Docked => bars > 0,
            Placement::Popped => characters > 0,
        }
}

/// A window on the taskbar and in the Alt-Tab list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Taskbar {
    /// It has its taskbar button.
    pub tab: bool,
    /// It is in the Alt-Tab list (Windows: it is not a tool window).
    pub switcher: bool,
}

impl Taskbar {
    /// As any window is.
    pub const NORMAL: Taskbar = Taskbar { tab: true, switcher: true };
}

impl Default for Taskbar {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// What a viewer knows of its window for [`taskbar`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowFacts {
    /// A host manages this viewer and is still there.
    pub linked: bool,
    /// The host asks this window off the taskbar and out of Alt-Tab
    /// (`Assignment::off_taskbar`).
    pub asked: bool,
    /// The host's active character.
    pub active: bool,
    /// The window has the keyboard.
    pub focused: bool,
    pub minimised: bool,
    /// The host minimises this window behind the active one (Economy).
    pub economy: bool,
}

/// What a managed viewer's window is on the taskbar and in Alt-Tab.
///
/// As any window unless a host that is there asks otherwise of a
/// background character whose window has not the keyboard: then it has no
/// taskbar button, and is out of Alt-Tab too, **except** when it is
/// minimised or about to be (Economy): Windows leaves a minimised tool
/// window as a small title bar above the taskbar, so such a window keeps
/// its style and only loses its button.
pub fn taskbar(window: WindowFacts) -> Taskbar {
    if !window.linked || !window.asked || window.active || window.focused {
        return Taskbar::NORMAL;
    }
    Taskbar {
        tab: false,
        switcher: window.minimised || window.economy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(slot: u8, presence: Presence, quick: Quick) -> Character {
        Character {
            slot,
            name: format!("demo-{}", slot + 1),
            real: None,
            colour: Colour::None,
            presence,
            quick,
            monitor: false,
        }
    }

    fn state() -> State {
        State {
            docked: true,
            characters: vec![
                character(0, Presence::Active, Quick::Show),
                character(1, Presence::Inactive, Quick::Switch),
                character(2, Presence::Out, Quick::LogIn),
                character(4, Presence::Out, Quick::Launch),
            ],
            inbox: 3,
            quick: Config::default().quick,
            voice_follows: true,
            ..State::default()
        }
    }

    #[test]
    fn the_defaults_are_docked_with_the_characters_and_attention() {
        let config = Config::default();
        assert_eq!(config.placement, Placement::Docked);
        assert!(!config.collapsed && !config.pinned && config.place.is_none());
        assert_eq!(config.quick.len(), MAX_QUICK);
        assert_eq!(config.quick[0], QuickAction::Character(0));
        assert_eq!(config.quick[4], QuickAction::Character(4));
        assert_eq!(config.quick[5], QuickAction::Attention);
        assert!(config.quick[6..].iter().all(|action| *action == QuickAction::Unused));
        assert_eq!(config.quick_labels, LabelMode::IconAndText);
        assert_eq!(config.toolbar_labels, LabelMode::IconOnly);
        assert!(config.one_taskbar_button && config.hide_host);
        assert!(!config.span_window, "over the world viewport by default");
        assert_eq!(config.clone().cleaned(), config);
    }

    #[test]
    fn a_label_mode_says_what_a_button_shows() {
        let shows = |mode: LabelMode| (mode.icon(), mode.text());
        assert_eq!(shows(LabelMode::IconOnly), (true, false));
        assert_eq!(shows(LabelMode::TextOnly), (false, true));
        assert_eq!(shows(LabelMode::IconAndText), (true, true));
        // without room: the icons alone, and a row of texts keeps them
        assert_eq!(LabelMode::IconAndText.narrow(), LabelMode::IconOnly);
        assert_eq!(LabelMode::IconOnly.narrow(), LabelMode::IconOnly);
        assert_eq!(LabelMode::TextOnly.narrow(), LabelMode::TextOnly);
        for (index, mode) in LabelMode::ALL.into_iter().enumerate() {
            assert_eq!(mode.index(), index);
        }
    }

    #[test]
    fn quick_actions_are_named_in_the_file_and_an_unknown_name_is_unused() {
        for action in QuickAction::all() {
            assert_eq!(QuickAction::parse(&action.key()), action, "{}", action.key());
        }
        assert_eq!(QuickAction::all().count(), 4 + MAX_SLOTS);
        assert_eq!(QuickAction::Character(0).key(), "character-1");
        for unknown in ["", "character-0", "character-6", "character-x", "restart-all", "Attention"] {
            assert_eq!(QuickAction::parse(unknown), QuickAction::Unused, "{unknown}");
        }
        let text = serde_json::to_string(&[QuickAction::Character(2), QuickAction::MuteBackground]).expect("json");
        assert_eq!(text, r#"["character-3","mute-background"]"#);
    }

    #[test]
    fn a_file_of_before_or_of_a_later_build_reads() {
        // before checkpoint 8: no bar at all
        let read: Config = serde_json::from_str("{}").expect("defaults");
        assert_eq!(read, Config::default());
        // a later build's names: each reads as this build's fallback
        let later = r#"{
            "placement": "floating",
            "quick": ["character-2", "restart-all", "attention"],
            "quick_labels": "tiny",
            "toolbar_labels": "text-only",
            "pinned": true,
            "something_new": 3
        }"#;
        let read = serde_json::from_str::<Config>(later).expect("reads").cleaned();
        assert_eq!(read.placement, Placement::Docked);
        assert_eq!(read.quick.len(), MAX_QUICK);
        assert_eq!(
            &read.quick[..3],
            &[QuickAction::Character(1), QuickAction::Unused, QuickAction::Attention]
        );
        assert_eq!(read.quick_labels, LabelMode::IconAndText);
        assert_eq!(read.toolbar_labels, LabelMode::TextOnly);
        assert!(read.pinned && read.one_taskbar_button && read.hide_host && !read.span_window);
        // what is written reads back the same
        let text = serde_json::to_string(&read).expect("json");
        assert!(
            text.contains("\"placement\":\"docked\"") && text.contains("\"toolbar_labels\":\"text-only\""),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Config>(&text).expect("json").cleaned(), read);
    }

    #[test]
    fn the_file_s_quick_actions_are_twelve_each_once_and_its_place_a_window_s() {
        let config = Config {
            quick: vec![QuickAction::Attention; 20],
            place: Some(Place {
                position: Some([40, 40]),
                size: [5, 5],
            }),
            ..Config::default()
        }
        .cleaned();
        assert_eq!(config.quick.len(), MAX_QUICK);
        assert_eq!(config.quick[0], QuickAction::Attention);
        assert!(config.quick[1..].iter().all(|action| *action == QuickAction::Unused));
        assert_eq!(config.place, None);
        // one row is a place the compact window can have
        let row = Place {
            position: Some([-1900, 12]),
            size: [900, 38],
        };
        let mut config = Config::default();
        assert!(config.placed(row));
        assert!(!config.placed(row), "where it was: nothing to save");
        assert!(!config.placed(Place {
            position: Some([1 << 20, 0]),
            size: [900, 38]
        }));
        assert_eq!(config.clone().cleaned().place, Some(row));
    }

    #[test]
    fn a_position_takes_an_action_from_the_position_that_had_it() {
        let mut config = Config::default();
        assert!(!config.set_quick(5, QuickAction::Attention), "as it was");
        assert!(!config.set_quick(MAX_QUICK, QuickAction::Attention), "no such position");
        assert!(config.set_quick(0, QuickAction::Attention));
        assert_eq!(config.quick[0], QuickAction::Attention);
        assert_eq!(config.quick[5], QuickAction::Unused, "each action shows once");
        assert!(config.set_quick(11, QuickAction::VoiceFollows));
        assert!(config.set_quick(1, QuickAction::Unused));
        assert!(config.set_quick(2, QuickAction::Unused), "several positions can be unused");
        assert_eq!(config.quick[11], QuickAction::VoiceFollows);
        assert_eq!(config.clone().cleaned(), config);
    }

    #[test]
    fn the_quick_actions_that_show_are_those_with_something_to_show() {
        let mut state = state();
        // slot 3 has no character: its position is left out
        let shown = state.buttons();
        assert_eq!(shown.len(), 5);
        assert!(matches!(shown[0], Button::Character(c) if c.slot == 0));
        assert!(matches!(shown[3], Button::Character(c) if c.slot == 4));
        assert_eq!(shown[4], Button::Attention(3));
        // Attention shows with nothing waiting too
        state.inbox = 0;
        assert_eq!(state.buttons()[4], Button::Attention(0));
        // the toggles, with what they are
        state.quick = vec![QuickAction::MuteBackground, QuickAction::Unused, QuickAction::VoiceFollows];
        state.mute_background = true;
        assert_eq!(state.buttons(), vec![Button::MuteBackground(true), Button::VoiceFollows(true)]);
        state.quick.clear();
        assert!(state.buttons().is_empty());
    }

    #[test]
    fn open_a_login_means_the_first_character_not_logged_in() {
        let mut state = state();
        assert_eq!(state.active().map(|c| c.slot), Some(0));
        assert_eq!(state.next_login().map(|c| c.slot), Some(2));
        assert_eq!(state.logins().map(|c| c.slot).collect::<Vec<_>>(), [2, 4]);
        // one that is closing has nothing to open
        state.characters[2].quick = Quick::Nothing;
        assert_eq!(state.next_login().map(|c| c.slot), Some(4));
        state.characters[3].presence = Presence::Inactive;
        state.characters[3].quick = Quick::Switch;
        assert_eq!(state.next_login(), None);
        // a monitor is for a character in the world behind the active one
        let watchable: Vec<u8> = state.characters.iter().filter(|c| c.watchable()).map(|c| c.slot).collect();
        assert_eq!(watchable, [1, 4]);
    }

    #[test]
    fn what_comes_down_the_pipe_is_bounded_and_cleaned() {
        let long = "x".repeat(500);
        let mut sent = state();
        sent.characters[0].name = format!("  Main\u{7}\n{long}");
        sent.characters[0].real = Some("\u{1b}".to_owned());
        sent.characters[1].real = Some(" Jane\tDoe ".to_owned());
        // a slot twice, a slot there is not, and too many quick actions
        sent.characters.push(character(1, Presence::Active, Quick::Show));
        sent.characters.push(character(9, Presence::Active, Quick::Show));
        sent.characters.swap(0, 3);
        sent.quick = vec![QuickAction::Attention; 40];
        let state = sent.cleaned();
        let slots: Vec<u8> = state.characters.iter().map(|c| c.slot).collect();
        assert_eq!(slots, [0, 1, 2, 4]);
        let first = &state.characters[0];
        assert!(first.name.starts_with("Mainx") && first.name.chars().count() <= MAX_NAME);
        assert_eq!(first.real, None, "nothing left of it: no real name");
        assert_eq!(state.characters[1].real.as_deref(), Some("JaneDoe"));
        assert_eq!(state.characters[1].presence, Presence::Inactive, "the first of a slot is kept");
        assert_eq!(state.quick.len(), MAX_QUICK);
    }

    #[test]
    fn the_strip_is_kept_only_while_the_bar_is_docked() {
        let mut state = state();
        assert_eq!(state.strip(), HEIGHT);
        state.collapsed = true;
        assert_eq!(state.strip(), HANDLE);
        state.docked = false;
        assert_eq!(state.strip(), 0.0);
        assert_eq!(State::default().strip(), 0.0);
    }

    #[test]
    fn a_request_names_a_slot_there_can_be() {
        assert!(Request::Pick(4).valid() && !Request::Pick(5).valid());
        assert!(Request::OpenLogin(None).valid() && !Request::OpenLogin(Some(200)).valid());
        assert!(Request::Monitor(0, true).valid() && !Request::Monitor(5, false).valid());
        assert!(Request::Show(Page::Quit).valid() && Request::Dock(false).valid());
        // only what brings a window forward asks for the foreground
        assert!(Request::Pick(0).shows_a_window() && Request::Chat(true).shows_a_window());
        assert!(Request::Show(Page::Bar).shows_a_window() && Request::Dock(false).shows_a_window());
        for quiet in [
            Request::Chat(false),
            Request::Monitor(1, false),
            Request::CloseMonitors,
            Request::MuteBackground(true),
            Request::VoiceFollows(false),
            Request::Dock(true),
            Request::Collapse(true),
            Request::Pin(true),
        ] {
            assert!(!quiet.shows_a_window(), "{quiet:?}");
        }
    }

    #[test]
    fn the_host_s_window_hides_only_while_a_bar_can_bring_it_back() {
        use Placement::{Docked, Popped};
        // docked: while a viewer shows the bar
        assert!(host_hidden(true, false, Docked, 1, 2));
        assert!(!host_hidden(true, false, Docked, 0, 2), "no viewer shows a bar: the host is there");
        // the user asked to see it, or does not want it hidden
        assert!(!host_hidden(true, true, Docked, 3, 3));
        assert!(!host_hidden(false, false, Docked, 3, 3));
        // popped out: the compact window is always there, once it has a
        // character to drive
        assert!(host_hidden(true, false, Popped, 0, 1));
        assert!(!host_hidden(true, false, Popped, 0, 0));
        assert!(!host_hidden(true, true, Popped, 2, 2));
    }

    #[test]
    fn a_window_is_as_usual_unless_the_host_asks_otherwise_of_a_background_character() {
        let background = WindowFacts {
            linked: true,
            asked: true,
            ..WindowFacts::default()
        };
        // a viewer started by hand, and one the host asks nothing of
        assert_eq!(taskbar(WindowFacts::default()), Taskbar::NORMAL);
        assert_eq!(
            taskbar(WindowFacts {
                asked: false,
                ..background
            }),
            Taskbar::NORMAL
        );
        // the host went away with its last word still held
        assert_eq!(
            taskbar(WindowFacts {
                linked: false,
                ..background
            }),
            Taskbar::NORMAL
        );
        // the active character, and a window the user went to before the
        // host followed
        assert_eq!(
            taskbar(WindowFacts {
                active: true,
                ..background
            }),
            Taskbar::NORMAL
        );
        assert_eq!(
            taskbar(WindowFacts {
                focused: true,
                ..background
            }),
            Taskbar::NORMAL
        );
        // behind the active one, Warm: off the taskbar and out of Alt-Tab
        assert_eq!(
            taskbar(background),
            Taskbar {
                tab: false,
                switcher: false
            }
        );
    }

    #[test]
    fn a_window_that_is_or_will_be_minimised_never_becomes_a_tool_window() {
        let background = WindowFacts {
            linked: true,
            asked: true,
            ..WindowFacts::default()
        };
        let button_only = Taskbar {
            tab: false,
            switcher: true,
        };
        // Economy: the host minimises it next
        assert_eq!(
            taskbar(WindowFacts {
                economy: true,
                ..background
            }),
            button_only
        );
        // minimised, whoever did it
        assert_eq!(
            taskbar(WindowFacts {
                minimised: true,
                ..background
            }),
            button_only
        );
        // and whatever else: never a tool window while minimised
        for (active, focused, asked, linked) in [
            (true, false, true, true),
            (false, true, true, true),
            (false, false, false, true),
            (false, false, true, false),
        ] {
            let window = WindowFacts {
                linked,
                asked,
                active,
                focused,
                minimised: true,
                economy: true,
            };
            assert!(taskbar(window).switcher, "{window:?}");
        }
    }
}
