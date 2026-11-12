//! The session host's global hotkeys (session-host plan, section 13,
//! checkpoint 4): which keys, for what, and what is wrong with a choice.
//! Pure: nothing here registers a key. The host's window does, on Windows
//! (`RegisterHotKey`), from the numbers [`Chord::modifiers`] and
//! [`Chord::vk`] give, and tells the user what Windows answered
//! ([`Outcome`]); elsewhere every hotkey reads [`State::Unavailable`].
//!
//! - **What a hotkey does** is the desk's (`host::Desk::hotkey`): a
//!   character's key is that character's quick action; "next" and
//!   "previous" go round the characters that run.
//! - **A hotkey is taken from every program** while the host is open, so a
//!   choice must not be a key the user types with: Ctrl or Alt is needed,
//!   and a second modifier with it unless the key is a function key. On
//!   keyboards with an AltGr key, Ctrl+Alt *is* AltGr: the host checks, on
//!   Windows, that a choice types no character on the current layout
//!   ([`Outcome::TypesCharacter`]).
//! - **Nothing is silent.** A text that is no hotkey, two actions on one
//!   key, a key another program holds: each is a [`State`] the window shows
//!   beside the hotkey.
//!
//! The settings ([`Config`]) hold the texts as the user sees them
//! ("Ctrl+Alt+1"), in `session-host.json`. Never a credential.

use crate::ipc::MAX_SLOTS;
use serde::{Deserialize, Serialize};
use std::fmt;

/// How many hotkeys there are: one per character, next and previous.
pub const ACTIONS: usize = MAX_SLOTS + 2;

/// The longest text kept for a hotkey, in characters.
pub const MAX_TEXT: usize = 40;

/// What a hotkey does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The quick action of the character in that slot (0 to
    /// [`MAX_SLOTS`] - 1).
    Character(usize),
    /// The next character that runs, after the active one.
    Next,
    /// The one before.
    Previous,
}

impl Action {
    /// Every action, in the order of [`Action::index`].
    pub fn all() -> impl Iterator<Item = Action> {
        (0..ACTIONS).filter_map(Action::from_index)
    }

    /// 0 to [`ACTIONS`] - 1: the characters first.
    pub fn index(self) -> usize {
        match self {
            Action::Character(slot) => slot.min(MAX_SLOTS - 1),
            Action::Next => MAX_SLOTS,
            Action::Previous => MAX_SLOTS + 1,
        }
    }

    pub fn from_index(index: usize) -> Option<Action> {
        match index {
            slot if slot < MAX_SLOTS => Some(Action::Character(slot)),
            MAX_SLOTS => Some(Action::Next),
            i if i == MAX_SLOTS + 1 => Some(Action::Previous),
            _ => None,
        }
    }

    /// The number the hotkey is registered under (1 to [`ACTIONS`]).
    pub fn id(self) -> i32 {
        self.index() as i32 + 1
    }

    /// The action of a number the system gives back; None for a number
    /// that is not one of ours.
    pub fn from_id(id: i64) -> Option<Action> {
        usize::try_from(id.checked_sub(1)?).ok().and_then(Action::from_index)
    }
}

/// The key of a hotkey, besides its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// 0 to 9, the row above the letters.
    Digit(u8),
    /// `b'A'` to `b'Z'`.
    Letter(u8),
    /// F1 to F24.
    Function(u8),
    /// 0 to 9 of the numeric keypad.
    Numpad(u8),
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
}

impl Key {
    fn parse(token: &str) -> Option<Key> {
        let lower = token.to_ascii_lowercase();
        let number = |text: &str| text.parse::<u8>().ok().filter(|_| text.len() <= 2);
        if let [byte] = lower.as_bytes() {
            return match byte {
                b'0'..=b'9' => Some(Key::Digit(byte - b'0')),
                b'a'..=b'z' => Some(Key::Letter(byte.to_ascii_uppercase())),
                _ => None,
            };
        }
        if let Some(n) = lower.strip_prefix("numpad").or_else(|| lower.strip_prefix("num")) {
            return number(n).filter(|n| *n <= 9).map(Key::Numpad);
        }
        if let Some(n) = lower.strip_prefix('f')
            && let Some(n) = number(n)
        {
            return (1..=24).contains(&n).then_some(Key::Function(n));
        }
        Some(match lower.as_str() {
            "left" => Key::Left,
            "right" => Key::Right,
            "up" => Key::Up,
            "down" => Key::Down,
            "pageup" | "pgup" => Key::PageUp,
            "pagedown" | "pgdn" | "pgdown" => Key::PageDown,
            "home" => Key::Home,
            "end" => Key::End,
            "insert" | "ins" => Key::Insert,
            _ => return None,
        })
    }

    /// The Windows virtual-key code.
    fn vk(self) -> u32 {
        match self {
            Key::Digit(d) => 0x30 + u32::from(d),
            Key::Letter(c) => u32::from(c),
            Key::Function(n) => 0x70 + u32::from(n) - 1,
            Key::Numpad(d) => 0x60 + u32::from(d),
            Key::PageUp => 0x21,
            Key::PageDown => 0x22,
            Key::End => 0x23,
            Key::Home => 0x24,
            Key::Left => 0x25,
            Key::Up => 0x26,
            Key::Right => 0x27,
            Key::Down => 0x28,
            Key::Insert => 0x2D,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Digit(d) => write!(f, "{d}"),
            Key::Letter(c) => write!(f, "{}", char::from(*c)),
            Key::Function(n) => write!(f, "F{n}"),
            Key::Numpad(d) => write!(f, "Num{d}"),
            Key::Left => f.write_str("Left"),
            Key::Right => f.write_str("Right"),
            Key::Up => f.write_str("Up"),
            Key::Down => f.write_str("Down"),
            Key::PageUp => f.write_str("PageUp"),
            Key::PageDown => f.write_str("PageDown"),
            Key::Home => f.write_str("Home"),
            Key::End => f.write_str("End"),
            Key::Insert => f.write_str("Insert"),
        }
    }
}

/// `MOD_ALT`, `MOD_CONTROL` and `MOD_SHIFT` of `RegisterHotKey`.
pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;

/// A hotkey: modifiers and one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

/// Why a text is not a hotkey.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Modifiers and no key.
    NoKey,
    /// More than one key.
    TwoKeys,
    /// A word that is neither a modifier nor a key the host knows (at most
    /// 16 characters of it).
    Unknown(String),
    /// Neither Ctrl nor Alt: a key the user types with.
    NoModifier,
    /// A letter or a digit with one modifier only: a shortcut of nearly
    /// every program (Ctrl+C, Alt+F).
    OneModifier,
    /// Alt+F4 closes windows.
    Reserved,
}

impl Chord {
    /// Reads "Ctrl+Alt+1": words joined by `+`, in any order and case,
    /// with at most one key.
    pub fn parse(text: &str) -> Result<Chord, ParseError> {
        let (mut ctrl, mut alt, mut shift, mut key) = (false, false, false, None);
        for token in text.split('+').map(str::trim) {
            match token.to_ascii_lowercase().as_str() {
                "" => {}
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                _ => {
                    let Some(found) = Key::parse(token) else {
                        return Err(ParseError::Unknown(token.chars().take(16).collect()));
                    };
                    if key.replace(found).is_some() {
                        return Err(ParseError::TwoKeys);
                    }
                }
            }
        }
        let key = key.ok_or(ParseError::NoKey)?;
        if !ctrl && !alt {
            return Err(ParseError::NoModifier);
        }
        let function = matches!(key, Key::Function(_));
        if !function && usize::from(ctrl) + usize::from(alt) + usize::from(shift) < 2 {
            return Err(ParseError::OneModifier);
        }
        if alt && !ctrl && !shift && key == Key::Function(4) {
            return Err(ParseError::Reserved);
        }
        Ok(Chord { ctrl, alt, shift, key })
    }

    /// The modifiers as `RegisterHotKey` takes them (without
    /// `MOD_NOREPEAT`, which the host adds).
    pub fn modifiers(&self) -> u32 {
        (if self.alt { MOD_ALT } else { 0 }) | (if self.ctrl { MOD_CONTROL } else { 0 }) | (if self.shift { MOD_SHIFT } else { 0 })
    }

    /// The Windows virtual-key code of the key.
    pub fn vk(&self) -> u32 {
        self.key.vk()
    }

    /// Ctrl and Alt together, which is what an AltGr key sends: the chord
    /// may be how the user types a character.
    pub fn may_be_altgr(&self) -> bool {
        self.ctrl && self.alt
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (held, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if held {
                f.write_str(name)?;
            }
        }
        write!(f, "{}", self.key)
    }
}

/// The hotkeys of the host's settings file, as texts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The hotkeys are registered at all.
    pub enabled: bool,
    /// One per slot; an empty text is no hotkey.
    pub characters: Vec<String>,
    pub next: String,
    pub previous: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            characters: (1..=MAX_SLOTS).map(|n| format!("Ctrl+Alt+{n}")).collect(),
            next: "Ctrl+Alt+PageDown".to_owned(),
            previous: "Ctrl+Alt+PageUp".to_owned(),
        }
    }
}

/// A text of the file bounded and without control characters.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect::<String>()
        .trim()
        .to_owned()
}

impl Config {
    /// The file as the host can use it: one bounded text per action.
    pub fn cleaned(mut self) -> Self {
        self.characters.truncate(MAX_SLOTS);
        self.characters.resize(MAX_SLOTS, String::new());
        for text in self.characters.iter_mut().chain([&mut self.next, &mut self.previous]) {
            *text = clean(text);
        }
        self
    }

    /// The text of an action's hotkey; empty: none.
    pub fn text(&self, action: Action) -> &str {
        match action {
            Action::Character(slot) => self.characters.get(slot).map_or("", String::as_str),
            Action::Next => &self.next,
            Action::Previous => &self.previous,
        }
    }

    /// Sets an action's hotkey from what the user typed: written the usual
    /// way when it is a hotkey ("ctrl + alt+1" becomes "Ctrl+Alt+1"), kept
    /// as typed otherwise so that the user can correct it. True when the
    /// settings changed.
    pub fn set(&mut self, action: Action, text: &str) -> bool {
        let text = match Chord::parse(text) {
            Ok(chord) => chord.to_string(),
            Err(_) => clean(text),
        };
        if self.characters.len() < MAX_SLOTS {
            self.characters.resize(MAX_SLOTS, String::new());
        }
        let place = match action {
            Action::Character(slot) => match self.characters.get_mut(slot) {
                Some(place) => place,
                None => return false,
            },
            Action::Next => &mut self.next,
            Action::Previous => &mut self.previous,
        };
        if *place == text {
            return false;
        }
        *place = text;
        true
    }

    /// What each action's text asks for, in the order of
    /// [`Action::index`], whether the hotkeys are enabled or not.
    pub fn plan(&self) -> [Wanted; ACTIONS] {
        let mut plan: [Wanted; ACTIONS] = std::array::from_fn(|_| Wanted::Unset);
        let mut taken: Vec<(Chord, Action)> = Vec::new();
        for action in Action::all() {
            let text = self.text(action).trim();
            if text.is_empty() {
                continue;
            }
            plan[action.index()] = match Chord::parse(text) {
                Err(error) => Wanted::Invalid(error),
                Ok(chord) => match taken.iter().find(|(other, _)| *other == chord) {
                    // the first action keeps the key
                    Some((_, first)) => Wanted::Duplicate(*first),
                    None => {
                        taken.push((chord, action));
                        Wanted::Chord(chord)
                    }
                },
            };
        }
        plan
    }

    /// The hotkeys to register: none while they are disabled.
    pub fn bindings(&self) -> Vec<(Action, Chord)> {
        if !self.enabled {
            return Vec::new();
        }
        let plan = self.plan();
        Action::all()
            .filter_map(|action| match plan[action.index()] {
                Wanted::Chord(chord) => Some((action, chord)),
                _ => None,
            })
            .collect()
    }
}

/// What an action's text asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wanted {
    /// No hotkey.
    Unset,
    Chord(Chord),
    /// The text is not a hotkey.
    Invalid(ParseError),
    /// The same key as that earlier action, which keeps it.
    Duplicate(Action),
}

/// What became of a hotkey the host tried to register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Registered,
    /// The system refused it: another program holds that key.
    Taken,
    /// With the current keyboard layout that chord types a character
    /// (AltGr): taking it from every program would take the character.
    TypesCharacter,
    /// Global hotkeys are not available on this system.
    Unavailable,
}

/// A hotkey as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Unset,
    /// A valid choice, not registered: the hotkeys are disabled.
    Disabled,
    Invalid(ParseError),
    Duplicate(Action),
    Registered,
    Taken,
    TypesCharacter,
    Unavailable,
}

impl State {
    /// `outcome`: what the host got when it tried (None: it has not).
    pub fn of(wanted: &Wanted, enabled: bool, outcome: Option<Outcome>) -> State {
        match wanted {
            Wanted::Unset => State::Unset,
            Wanted::Invalid(error) => State::Invalid(error.clone()),
            Wanted::Duplicate(first) => State::Duplicate(*first),
            Wanted::Chord(_) if !enabled => State::Disabled,
            Wanted::Chord(_) => match outcome {
                Some(Outcome::Registered) => State::Registered,
                Some(Outcome::Taken) => State::Taken,
                Some(Outcome::TypesCharacter) => State::TypesCharacter,
                Some(Outcome::Unavailable) | None => State::Unavailable,
            },
        }
    }

    /// The hotkey does not work and the user should know.
    pub fn problem(&self) -> bool {
        matches!(self, State::Invalid(_) | State::Duplicate(_) | State::Taken | State::TypesCharacter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).expect("a hotkey")
    }

    #[test]
    fn actions_have_stable_numbers() {
        let all: Vec<Action> = Action::all().collect();
        assert_eq!(all.len(), ACTIONS);
        assert_eq!(all[0], Action::Character(0));
        assert_eq!(all[MAX_SLOTS - 1], Action::Character(MAX_SLOTS - 1));
        assert_eq!(&all[MAX_SLOTS..], [Action::Next, Action::Previous]);
        for (index, action) in all.iter().enumerate() {
            assert_eq!(action.index(), index);
            assert_eq!(Action::from_index(index), Some(*action));
            assert_eq!(action.id(), index as i32 + 1);
            assert_eq!(Action::from_id(i64::from(action.id())), Some(*action));
        }
        // what the system may hand back that is not ours
        for id in [0, -1, ACTIONS as i64 + 1, i64::MAX, i64::MIN] {
            assert_eq!(Action::from_id(id), None, "{id}");
        }
        assert_eq!(Action::from_index(ACTIONS), None);
    }

    #[test]
    fn hotkeys_are_read_in_any_order_and_case_and_written_one_way() {
        let c = chord("Ctrl+Alt+1");
        assert_eq!((c.ctrl, c.alt, c.shift, c.key), (true, true, false, Key::Digit(1)));
        for same in ["ctrl+alt+1", " ALT + Control + 1 ", "1+alt+ctrl", "Alt+Ctrl+1"] {
            assert_eq!(chord(same), c, "{same}");
            assert_eq!(chord(same).to_string(), "Ctrl+Alt+1");
        }
        for (text, written) in [
            ("ctrl+shift+k", "Ctrl+Shift+K"),
            ("alt+shift+f12", "Alt+Shift+F12"),
            ("ctrl+f5", "Ctrl+F5"),
            ("Ctrl+Alt+pgdn", "Ctrl+Alt+PageDown"),
            ("ctrl+alt+PGUP", "Ctrl+Alt+PageUp"),
            ("ctrl+alt+left", "Ctrl+Alt+Left"),
            ("ctrl+alt+num3", "Ctrl+Alt+Num3"),
            ("ctrl+alt+numpad0", "Ctrl+Alt+Num0"),
            ("ctrl+alt+shift+ins", "Ctrl+Alt+Shift+Insert"),
            ("ctrl+alt+f24", "Ctrl+Alt+F24"),
        ] {
            let read = chord(text);
            assert_eq!(read.to_string(), written);
            // what is written reads back the same
            assert_eq!(chord(written), read);
        }
    }

    #[test]
    fn a_text_that_is_no_hotkey_is_refused_with_why() {
        use ParseError::*;
        for (text, why) in [
            ("Ctrl+Alt", NoKey),
            ("Ctrl+", NoKey),
            ("", NoKey),
            ("Ctrl+Alt+1+2", TwoKeys),
            ("Ctrl+Alt+Delete", Unknown("Delete".into())),
            ("Win+1", Unknown("Win".into())),
            ("Ctrl+Alt+F25", Unknown("F25".into())),
            ("Ctrl+Alt+F0", Unknown("F0".into())),
            ("Ctrl+Alt+Num10", Unknown("Num10".into())),
            ("Ctrl+Alt+é", Unknown("é".into())),
            ("Ctrl+Alt+Tab", Unknown("Tab".into())),
            ("hello", Unknown("hello".into())),
            // keys the user types with, and other programs' shortcuts
            ("1", NoModifier),
            ("F5", NoModifier),
            ("Shift+A", NoModifier),
            ("Shift+F5", NoModifier),
            ("Ctrl+C", OneModifier),
            ("Alt+1", OneModifier),
            ("Ctrl+Left", OneModifier),
            ("Alt+F4", Reserved),
        ] {
            assert_eq!(Chord::parse(text), Err(why), "{text}");
        }
        // a long word is cut before it is shown
        let Err(Unknown(word)) = Chord::parse(&"x".repeat(500)) else {
            panic!("refused");
        };
        assert_eq!(word.chars().count(), 16);
        // a function key needs one modifier only, and Alt+F4 alone is kept
        assert!(Chord::parse("Alt+F5").is_ok());
        assert!(Chord::parse("Ctrl+Alt+F4").is_ok());
    }

    #[test]
    fn the_numbers_windows_takes() {
        let c = chord("Ctrl+Alt+1");
        assert_eq!((c.modifiers(), c.vk()), (MOD_CONTROL | MOD_ALT, 0x31));
        assert_eq!(chord("Ctrl+Shift+A").modifiers(), MOD_CONTROL | MOD_SHIFT);
        assert_eq!(chord("Alt+Shift+Z").vk(), 0x5A);
        for (text, vk) in [
            ("Ctrl+Alt+0", 0x30),
            ("Ctrl+Alt+9", 0x39),
            ("Ctrl+Alt+A", 0x41),
            ("Ctrl+F1", 0x70),
            ("Ctrl+F12", 0x7B),
            ("Ctrl+F24", 0x87),
            ("Ctrl+Alt+Num0", 0x60),
            ("Ctrl+Alt+Num9", 0x69),
            ("Ctrl+Alt+PageUp", 0x21),
            ("Ctrl+Alt+PageDown", 0x22),
            ("Ctrl+Alt+End", 0x23),
            ("Ctrl+Alt+Home", 0x24),
            ("Ctrl+Alt+Left", 0x25),
            ("Ctrl+Alt+Up", 0x26),
            ("Ctrl+Alt+Right", 0x27),
            ("Ctrl+Alt+Down", 0x28),
            ("Ctrl+Alt+Insert", 0x2D),
        ] {
            assert_eq!(chord(text).vk(), vk, "{text}");
        }
        assert!(chord("Ctrl+Alt+3").may_be_altgr());
        assert!(chord("Ctrl+Alt+Shift+3").may_be_altgr());
        assert!(!chord("Ctrl+Shift+3").may_be_altgr());
    }

    #[test]
    fn the_defaults_are_ctrl_alt_one_to_five_next_and_previous() {
        let config = Config::default();
        assert!(config.enabled);
        let bindings = config.bindings();
        assert_eq!(bindings.len(), ACTIONS);
        for (slot, (action, chord)) in bindings.iter().take(MAX_SLOTS).enumerate() {
            assert_eq!(*action, Action::Character(slot));
            assert_eq!(chord.to_string(), format!("Ctrl+Alt+{}", slot + 1));
        }
        assert_eq!(config.text(Action::Next), "Ctrl+Alt+PageDown");
        assert_eq!(config.text(Action::Previous), "Ctrl+Alt+PageUp");
        assert_eq!(config.clone().cleaned(), config);
        // disabled: nothing is registered, the choices stay
        let off = Config { enabled: false, ..config };
        assert!(off.bindings().is_empty());
        assert!(matches!(off.plan()[0], Wanted::Chord(_)));
    }

    #[test]
    fn the_settings_file_is_cleaned_and_holds_only_texts() {
        let read: Config = serde_json::from_str(r#"{"characters":["ctrl+alt+7"],"next":"x\u0000\ny","more":1}"#).expect("read");
        let read = read.cleaned();
        assert!(read.enabled, "what the file does not say is the default");
        assert_eq!(read.characters.len(), MAX_SLOTS);
        assert_eq!(read.characters[0], "ctrl+alt+7");
        assert_eq!(read.characters[1], "");
        assert_eq!(read.next, "xy");
        assert_eq!(read.previous, "Ctrl+Alt+PageUp");
        let long = Config {
            characters: vec!["k".repeat(500); 9],
            ..Config::default()
        }
        .cleaned();
        assert_eq!(long.characters.len(), MAX_SLOTS);
        assert!(long.characters.iter().all(|t| t.chars().count() == MAX_TEXT));
        // written back: texts and a flag, nothing else
        let written = serde_json::to_value(Config::default()).expect("written");
        let mut keys: Vec<&str> = written.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["characters", "enabled", "next", "previous"]);
    }

    #[test]
    fn a_choice_is_written_the_usual_way_or_kept_to_be_corrected() {
        let mut config = Config::default();
        assert!(config.set(Action::Character(1), " ctrl + shift+f2 "));
        assert_eq!(config.text(Action::Character(1)), "Ctrl+Shift+F2");
        assert!(!config.set(Action::Character(1), "CTRL+SHIFT+F2"), "the same hotkey");
        assert!(config.set(Action::Next, "ctrl+banana"));
        assert_eq!(config.text(Action::Next), "ctrl+banana");
        assert_eq!(
            config.plan()[Action::Next.index()],
            Wanted::Invalid(ParseError::Unknown("banana".into()))
        );
        assert!(config.set(Action::Previous, ""));
        assert_eq!(config.plan()[Action::Previous.index()], Wanted::Unset);
        // a file with fewer texts than slots still takes a choice
        let mut short = Config {
            characters: Vec::new(),
            ..Config::default()
        };
        assert!(short.set(Action::Character(4), "Ctrl+Alt+5"));
        assert_eq!(short.text(Action::Character(4)), "Ctrl+Alt+5");
        assert!(!short.set(Action::Character(MAX_SLOTS), "Ctrl+Alt+6"));
    }

    #[test]
    fn two_actions_on_one_key_the_first_keeps_it_and_the_other_is_told() {
        let mut config = Config::default();
        assert!(config.set(Action::Character(3), "alt+ctrl+1"));
        assert!(config.set(Action::Previous, "Ctrl+Alt+1"));
        let plan = config.plan();
        assert!(matches!(plan[0], Wanted::Chord(_)));
        assert_eq!(plan[3], Wanted::Duplicate(Action::Character(0)));
        assert_eq!(plan[Action::Previous.index()], Wanted::Duplicate(Action::Character(0)));
        let bindings = config.bindings();
        assert_eq!(bindings.len(), ACTIONS - 2);
        assert!(bindings.iter().all(|(action, _)| *action != Action::Character(3)));
        // every chord registered is registered once
        for (i, (_, a)) in bindings.iter().enumerate() {
            assert!(bindings.iter().skip(i + 1).all(|(_, b)| a != b));
        }
    }

    #[test]
    fn every_hotkey_has_a_state_to_show_and_failures_are_problems() {
        let wanted = Wanted::Chord(chord("Ctrl+Alt+1"));
        assert_eq!(State::of(&wanted, true, Some(Outcome::Registered)), State::Registered);
        assert_eq!(State::of(&wanted, true, Some(Outcome::Taken)), State::Taken);
        assert_eq!(State::of(&wanted, true, Some(Outcome::TypesCharacter)), State::TypesCharacter);
        assert_eq!(State::of(&wanted, true, Some(Outcome::Unavailable)), State::Unavailable);
        assert_eq!(State::of(&wanted, true, None), State::Unavailable);
        assert_eq!(State::of(&wanted, false, Some(Outcome::Registered)), State::Disabled);
        assert_eq!(State::of(&Wanted::Unset, true, None), State::Unset);
        let invalid = State::of(&Wanted::Invalid(ParseError::NoKey), false, None);
        assert_eq!(invalid, State::Invalid(ParseError::NoKey));
        let duplicate = State::of(&Wanted::Duplicate(Action::Next), true, Some(Outcome::Registered));
        assert_eq!(duplicate, State::Duplicate(Action::Next));
        for state in [State::Taken, State::TypesCharacter, invalid, duplicate] {
            assert!(state.problem(), "{state:?}");
        }
        for state in [State::Unset, State::Disabled, State::Registered, State::Unavailable] {
            assert!(!state.problem(), "{state:?}");
        }
    }
}
