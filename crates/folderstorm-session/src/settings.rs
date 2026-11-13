//! Checkpoint 10 of the session host, its settings in one window
//! (session-host plan, section 15): the sections of that window, and the
//! rules of the settings it adds. All pure:
//!
//! - **What the chats show** ([`ChatShown`]): which kinds of conversation
//!   (nearby, direct IMs, groups, conferences) and whether the viewer's
//!   own notices are in the shared chat and the monitors' chat.
//! - **What Attention lists** ([`Attention`]): which kinds of question,
//!   and in which order; a character left out of it ([`Attention::lists`]).
//! - **Reading** ([`Reading`], [`AutoRead`]): what is shown in a window
//!   that has the keyboard is read, or not; the active character's chats
//!   are read in the host once it has been in front for a while.
//! - **A character's alerts** ([`Alerts`]): which kinds of message sound
//!   the host's alert while another character is active.
//!
//! Every field reads as its default when it is absent from the file, so a
//! file of an earlier checkpoint reads as it was, with these defaults.

use crate::ipc::{AttentionKind, LineKind, SessionKind};
use crate::panels::Place;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The sections of the settings window, in its order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    /// The bar, its labels and quick actions, the host's windows.
    #[default]
    Toolbar,
    /// One character's own settings.
    Characters,
    /// What the chats show, what Attention lists, reading.
    Chat,
    /// Voice, background sound, the alert.
    Sound,
    Monitors,
    Hotkeys,
    /// Files, windows, diagnostics, credits.
    Advanced,
    /// Where the viewer's own updates and support settings are.
    Updates,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Toolbar,
        Section::Characters,
        Section::Chat,
        Section::Sound,
        Section::Monitors,
        Section::Hotkeys,
        Section::Advanced,
        Section::Updates,
    ];

    /// Its name in `AURORA_DEMO_SESSION_HOST_SETTINGS`.
    pub fn key(self) -> &'static str {
        match self {
            Section::Toolbar => "toolbar",
            Section::Characters => "characters",
            Section::Chat => "chat",
            Section::Sound => "sound",
            Section::Monitors => "monitors",
            Section::Hotkeys => "hotkeys",
            Section::Advanced => "advanced",
            Section::Updates => "updates",
        }
    }

    /// The section a name says (its key, a few other words for it, any
    /// case); None for anything else.
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        let found = match name.as_str() {
            "bar" | "quick" | "windows" => Section::Toolbar,
            "character" => Section::Characters,
            "attention" | "reading" | "chats" => Section::Chat,
            "voice" | "audio" | "alerts" => Section::Sound,
            "monitor" => Section::Monitors,
            "keys" | "shortcuts" => Section::Hotkeys,
            "diagnostics" | "files" => Section::Advanced,
            "support" | "update" => Section::Updates,
            "1" | "yes" | "on" | "true" => Section::default(),
            other => return Section::ALL.into_iter().find(|section| section.key() == other),
        };
        Some(found)
    }

    pub fn index(self) -> usize {
        Section::ALL.iter().position(|section| *section == self).unwrap_or(0)
    }
}

/// What the shared chat and the monitors' chat show, for every character
/// (`session-host.json`, `chat_shown`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatShown {
    pub nearby: bool,
    pub ims: bool,
    pub groups: bool,
    pub conferences: bool,
    /// The viewer's and the grid's own lines ([`LineKind::System`]).
    pub notices: bool,
}

impl Default for ChatShown {
    fn default() -> Self {
        Self {
            nearby: true,
            ims: true,
            groups: true,
            conferences: true,
            notices: true,
        }
    }
}

impl ChatShown {
    /// A conversation of that kind is shown.
    pub fn thread(&self, kind: SessionKind) -> bool {
        match kind {
            SessionKind::Nearby => self.nearby,
            SessionKind::Resident => self.ims,
            SessionKind::Group => self.groups,
            SessionKind::Conference => self.conferences,
        }
    }

    /// A line of that kind is shown, in a conversation that is.
    pub fn line(&self, kind: LineKind) -> bool {
        kind != LineKind::System || self.notices
    }
}

/// Reading (`session-host.json`, `reading`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reading {
    /// What a window of the host that has the keyboard shows is read
    /// (checkpoint 6's rule). Off: nothing is read by being shown.
    pub mark_displayed: bool,
    /// The active character's chats are read in the host once it has
    /// been in front for `auto_read_after_s`.
    pub auto_read: bool,
    pub auto_read_after_s: u32,
}

/// The least and the most delay before the active character is read.
pub const AUTO_READ_MIN_S: u32 = 5;
pub const AUTO_READ_MAX_S: u32 = 3600;

impl Default for Reading {
    fn default() -> Self {
        Self {
            mark_displayed: true,
            auto_read: true,
            auto_read_after_s: 60,
        }
    }
}

impl Reading {
    pub fn cleaned(mut self) -> Self {
        self.auto_read_after_s = self.auto_read_after_s.clamp(AUTO_READ_MIN_S, AUTO_READ_MAX_S);
        self
    }

    pub fn delay(&self) -> Duration {
        Duration::from_secs(u64::from(self.auto_read_after_s.clamp(AUTO_READ_MIN_S, AUTO_READ_MAX_S)))
    }
}

/// When the active character is read ([`Reading::auto_read`]): it has
/// been in front, without a change, for the delay. Told the active
/// character and the time at each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoRead {
    since: Option<(usize, Instant)>,
}

impl AutoRead {
    /// The character to read now, if any: the active one, once it has been
    /// in front for the delay, and at every tick after while it stays.
    /// Another character in front, or none, starts the count again.
    pub fn tick(&mut self, active: Option<usize>, now: Instant, reading: &Reading) -> Option<usize> {
        let Some(slot) = active else {
            self.since = None;
            return None;
        };
        let since = match self.since {
            Some((held, since)) if held == slot => since,
            _ => {
                self.since = Some((slot, now));
                now
            }
        };
        (reading.auto_read && now.saturating_duration_since(since) >= reading.delay()).then_some(slot)
    }
}

/// What Attention lists, for every character (`session-host.json`,
/// `attention`). It lists questions only: no chats (the human's answer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attention {
    /// Friendship and group offers, scripts' dialogs, links, calls and
    /// the rest of what a viewer asks.
    pub notices: bool,
    pub teleports: bool,
    pub permissions: bool,
    pub inventory: bool,
    /// The latest first; off: what waited the longest first.
    pub newest_first: bool,
}

impl Default for Attention {
    fn default() -> Self {
        Self {
            notices: true,
            teleports: true,
            permissions: true,
            inventory: true,
            newest_first: false,
        }
    }
}

impl Attention {
    /// A question of that kind is listed.
    pub fn kind(&self, kind: AttentionKind) -> bool {
        match kind {
            AttentionKind::Teleport => self.teleports,
            AttentionKind::Permissions => self.permissions,
            AttentionKind::Inventory => self.inventory,
            AttentionKind::Friendship
            | AttentionKind::Group
            | AttentionKind::Script
            | AttentionKind::Link
            | AttentionKind::Call
            | AttentionKind::Other => self.notices,
        }
    }

    /// A question of that kind, of a character `included` in Attention or
    /// not, is listed.
    pub fn lists(&self, included: bool, kind: AttentionKind) -> bool {
        included && self.kind(kind)
    }
}

/// What sounds the host's alert for a character while another one is
/// active (`session-host.json`, each character's `alerts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alerts {
    pub ims: bool,
    pub groups: bool,
    pub conferences: bool,
    /// An offer shown as a card in a conversation.
    pub notifications: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Self {
            ims: true,
            groups: true,
            conferences: true,
            notifications: true,
        }
    }
}

impl Alerts {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// A new line of that kind, in a conversation of that kind, sounds
    /// the alert. Nearby chat never does (as before), nor the character's
    /// own lines, an object's or the viewer's.
    pub fn sounds(&self, conversation: SessionKind, line: LineKind) -> bool {
        match (conversation, line) {
            (SessionKind::Nearby, _) => false,
            (_, LineKind::Card) => self.notifications,
            (SessionKind::Resident, LineKind::Said) => self.ims,
            (SessionKind::Group, LineKind::Said) => self.groups,
            (SessionKind::Conference, LineKind::Said) => self.conferences,
            _ => false,
        }
    }
}

/// The settings window (`session-host.json`, `settings_window`): where it
/// was and the section it showed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub place: Option<Place>,
    pub section: Section,
}

impl Window {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn cleaned(mut self) -> Self {
        self.place = self.place.filter(Place::valid);
        self
    }
}

/// The settings window's first size, in points, and its least.
pub const WINDOW_SIZE: [u32; 2] = [860, 640];
pub const WINDOW_MIN_SIZE: [u32; 2] = [480, 360];

/// `true` for a serde field whose default is on.
pub fn yes() -> bool {
    true
}

/// For `skip_serializing_if` of a field whose default is on.
pub fn is_yes(value: &bool) -> bool {
    *value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_key_that_reads_back_and_a_place() {
        for (i, section) in Section::ALL.into_iter().enumerate() {
            assert_eq!(Section::parse(section.key()), Some(section));
            assert_eq!(Section::parse(&format!(" {} ", section.key().to_uppercase())), Some(section));
            assert_eq!(section.index(), i);
        }
        assert_eq!(Section::parse("support"), Some(Section::Updates));
        assert_eq!(Section::parse("1"), Some(Section::Toolbar));
        assert_eq!(Section::parse("theme"), None);
        assert_eq!(Section::parse(""), None);
    }

    #[test]
    fn the_chats_show_what_is_chosen_and_everything_at_first() {
        let all = ChatShown::default();
        for kind in [
            SessionKind::Nearby,
            SessionKind::Resident,
            SessionKind::Group,
            SessionKind::Conference,
        ] {
            assert!(all.thread(kind));
        }
        assert!(all.line(LineKind::System));
        let some = ChatShown {
            groups: false,
            notices: false,
            ..ChatShown::default()
        };
        assert!(!some.thread(SessionKind::Group));
        assert!(some.thread(SessionKind::Resident) && some.thread(SessionKind::Conference));
        assert!(!some.line(LineKind::System));
        for kind in [LineKind::Said, LineKind::Own, LineKind::Object, LineKind::Card] {
            assert!(some.line(kind), "{kind:?}");
        }
    }

    #[test]
    fn attention_lists_the_kinds_chosen_of_the_characters_included() {
        let all = Attention::default();
        let kinds = [
            AttentionKind::Teleport,
            AttentionKind::Friendship,
            AttentionKind::Inventory,
            AttentionKind::Group,
            AttentionKind::Script,
            AttentionKind::Permissions,
            AttentionKind::Link,
            AttentionKind::Call,
            AttentionKind::Other,
        ];
        assert!(kinds.iter().all(|kind| all.lists(true, *kind)));
        assert!(kinds.iter().all(|kind| !all.lists(false, *kind)), "a character left out");
        let no_teleports = Attention { teleports: false, ..all };
        assert!(!no_teleports.kind(AttentionKind::Teleport));
        assert!(no_teleports.kind(AttentionKind::Permissions) && no_teleports.kind(AttentionKind::Inventory));
        let no_notices = Attention { notices: false, ..all };
        for kind in [
            AttentionKind::Friendship,
            AttentionKind::Script,
            AttentionKind::Call,
            AttentionKind::Other,
        ] {
            assert!(!no_notices.kind(kind), "{kind:?}");
        }
        assert!(no_notices.kind(AttentionKind::Teleport));
    }

    #[test]
    fn the_active_character_is_read_after_the_delay_and_for_as_long_as_it_stays() {
        let reading = Reading::default();
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut auto = AutoRead::default();
        assert_eq!(auto.tick(Some(0), at(0), &reading), None);
        assert_eq!(auto.tick(Some(0), at(59), &reading), None);
        assert_eq!(auto.tick(Some(0), at(60), &reading), Some(0));
        assert_eq!(auto.tick(Some(0), at(300), &reading), Some(0), "while it stays in front");
        // another character in front: its own count
        assert_eq!(auto.tick(Some(1), at(301), &reading), None);
        assert_eq!(auto.tick(Some(1), at(360), &reading), None);
        assert_eq!(auto.tick(Some(1), at(361), &reading), Some(1));
        // nobody in front, then the first again: from the start
        assert_eq!(auto.tick(None, at(362), &reading), None);
        assert_eq!(auto.tick(Some(0), at(363), &reading), None);
        assert_eq!(auto.tick(Some(0), at(422), &reading), None);
        assert_eq!(auto.tick(Some(0), at(423), &reading), Some(0));
        // off: never
        let off = Reading {
            auto_read: false,
            ..reading
        };
        assert_eq!(auto.tick(Some(0), at(10_000), &off), None);
        // the delay is held between its bounds
        let short = Reading {
            auto_read_after_s: 0,
            ..reading
        };
        assert_eq!(short.delay(), Duration::from_secs(u64::from(AUTO_READ_MIN_S)));
        assert_eq!(short.cleaned().auto_read_after_s, AUTO_READ_MIN_S);
        let long = Reading {
            auto_read_after_s: 1 << 30,
            ..reading
        };
        assert_eq!(long.cleaned().auto_read_after_s, AUTO_READ_MAX_S);
    }

    #[test]
    fn an_alert_sounds_for_the_kinds_chosen_and_never_for_nearby_chat() {
        let all = Alerts::default();
        assert!(all.sounds(SessionKind::Resident, LineKind::Said));
        assert!(all.sounds(SessionKind::Group, LineKind::Said));
        assert!(all.sounds(SessionKind::Conference, LineKind::Said));
        assert!(all.sounds(SessionKind::Resident, LineKind::Card));
        assert!(!all.sounds(SessionKind::Nearby, LineKind::Said));
        assert!(!all.sounds(SessionKind::Nearby, LineKind::Card));
        for kind in [LineKind::Own, LineKind::System, LineKind::Object] {
            assert!(!all.sounds(SessionKind::Resident, kind), "{kind:?}");
        }
        let quiet_groups = Alerts {
            groups: false,
            notifications: false,
            ..all
        };
        assert!(!quiet_groups.sounds(SessionKind::Group, LineKind::Said));
        assert!(!quiet_groups.sounds(SessionKind::Resident, LineKind::Card));
        assert!(quiet_groups.sounds(SessionKind::Resident, LineKind::Said));
    }

    #[test]
    fn absent_fields_read_as_the_defaults() {
        let chat: ChatShown = serde_json::from_str("{}").expect("reads");
        assert_eq!(chat, ChatShown::default());
        let reading: Reading = serde_json::from_str(r#"{"auto_read": false}"#).expect("reads");
        assert_eq!(
            reading,
            Reading {
                auto_read: false,
                ..Reading::default()
            }
        );
        let attention: Attention = serde_json::from_str(r#"{"newest_first": true}"#).expect("reads");
        assert!(attention.newest_first && attention.teleports);
        let alerts: Alerts = serde_json::from_str(r#"{"groups": false}"#).expect("reads");
        assert!(!alerts.groups && alerts.ims);
        let window: Window = serde_json::from_str(r#"{"section": "Hotkeys"}"#).expect("reads");
        assert_eq!(window.section, Section::Hotkeys);
    }
}
