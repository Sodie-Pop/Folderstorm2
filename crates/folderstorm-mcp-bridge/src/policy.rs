//! Who may do what through the bridge: permission classes with an Allow /
//! Ask / Never level each, the presets of Folderstorm (C++)'s
//! `doc/plans/assistant-permission-presets.md`, and the settings saved in
//! `settings.json` (`local_assistant`).

use serde::{Deserialize, Serialize};

/// What a tool does, for the permission it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// Inventory and outfit reads. Allow or Never only.
    Read,
    /// Phase 2: rename, describe, create folders.
    Edit,
    /// Phase 2: move, copy, link.
    Move,
    /// Phase 2: wear, take off, detach.
    Wear,
    /// Phase 2: move to Trash, restore from it. Permanent delete and Empty
    /// Trash are never a class: always refused.
    Trash,
}

impl Class {
    pub const ALL: [Class; 5] = [Class::Read, Class::Edit, Class::Move, Class::Wear, Class::Trash];

    /// Whether the class can ask the user before each call (every class
    /// that changes something; reading does not ask).
    pub fn can_ask(self) -> bool {
        self != Class::Read
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Allow,
    Ask,
    #[default]
    Never,
}

/// One level per class. Unknown or missing levels read as the safe default
/// of the field (Read: Allow, the others: Never).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    pub read: Level,
    pub edit: Level,
    #[serde(rename = "move")]
    pub move_: Level,
    pub wear: Level,
    pub trash: Level,
}

impl Default for Permissions {
    /// Read only: the preset a new user starts with.
    fn default() -> Self {
        Preset::ReadOnly.permissions()
    }
}

impl Permissions {
    pub fn level(&self, class: Class) -> Level {
        let level = match class {
            Class::Read => self.read,
            Class::Edit => self.edit,
            Class::Move => self.move_,
            Class::Wear => self.wear,
            Class::Trash => self.trash,
        };
        // Read has no Ask: a hand-edited file cannot give it one
        if level == Level::Ask && !class.can_ask() { Level::Never } else { level }
    }

    pub fn set(&mut self, class: Class, level: Level) {
        let level = if level == Level::Ask && !class.can_ask() { Level::Never } else { level };
        match class {
            Class::Read => self.read = level,
            Class::Edit => self.edit = level,
            Class::Move => self.move_ = level,
            Class::Wear => self.wear = level,
            Class::Trash => self.trash = level,
        }
    }

    /// The preset these levels match, Custom for any other combination.
    pub fn preset(&self) -> Preset {
        [Preset::ReadOnly, Preset::AskBeforeChanges]
            .into_iter()
            .find(|preset| Class::ALL.iter().all(|c| self.level(*c) == preset.permissions().level(*c)))
            .unwrap_or(Preset::Custom)
    }

    /// Whether a call of `class` may run now. Phase 1 has no approval
    /// dialog: a class set to Ask is refused like Never until phase 2 adds
    /// the dialog.
    pub fn check(&self, class: Class) -> Result<(), Level> {
        match self.level(class) {
            Level::Allow => Ok(()),
            level => Err(level),
        }
    }
}

/// The main choice of Preferences › Local assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Reads only; every change refused.
    ReadOnly,
    /// Reads; every change asks first.
    AskBeforeChanges,
    /// Any other combination: the individual levels show.
    Custom,
}

impl Preset {
    /// The levels of a preset; Custom keeps nothing of its own and gives
    /// Read only's.
    pub fn permissions(self) -> Permissions {
        let changes = match self {
            Preset::AskBeforeChanges => Level::Ask,
            Preset::ReadOnly | Preset::Custom => Level::Never,
        };
        Permissions {
            read: Level::Allow,
            edit: changes,
            move_: changes,
            wear: changes,
            trash: changes,
        }
    }
}

/// The saved settings (`settings.json` › `local_assistant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAssistant {
    /// "Allow local assistant access". Off by default.
    pub enabled: bool,
    pub permissions: Permissions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_and_read_only_by_default() {
        let d = LocalAssistant::default();
        assert!(!d.enabled);
        assert_eq!(d.permissions.preset(), Preset::ReadOnly);
        assert_eq!(d.permissions.check(Class::Read), Ok(()));
        for class in [Class::Edit, Class::Move, Class::Wear, Class::Trash] {
            assert_eq!(d.permissions.check(class), Err(Level::Never));
        }
    }

    #[test]
    fn presets_round_trip_and_anything_else_is_custom() {
        for preset in [Preset::ReadOnly, Preset::AskBeforeChanges] {
            assert_eq!(preset.permissions().preset(), preset);
        }
        let mut p = Preset::AskBeforeChanges.permissions();
        p.set(Class::Trash, Level::Never);
        assert_eq!(p.preset(), Preset::Custom);
        let mut p = Preset::ReadOnly.permissions();
        p.set(Class::Read, Level::Never);
        assert_eq!(p.preset(), Preset::Custom);
    }

    #[test]
    fn read_set_to_never_refuses_reads() {
        let mut p = Permissions::default();
        p.set(Class::Read, Level::Never);
        assert_eq!(p.check(Class::Read), Err(Level::Never));
    }

    #[test]
    fn read_cannot_ask() {
        let mut p = Permissions::default();
        p.set(Class::Read, Level::Ask);
        assert_eq!(p.level(Class::Read), Level::Never);
        let edited: Permissions = serde_json::from_str(r#"{"read":"ask"}"#).expect("parsed");
        assert_eq!(edited.level(Class::Read), Level::Never);
    }

    #[test]
    fn ask_is_refused_until_there_is_a_dialog() {
        let p = Preset::AskBeforeChanges.permissions();
        assert_eq!(p.check(Class::Wear), Err(Level::Ask));
    }

    #[test]
    fn missing_and_unknown_fields_take_safe_defaults() {
        let s: LocalAssistant = serde_json::from_str("{}").expect("parsed");
        assert_eq!(s, LocalAssistant::default());
        let s: LocalAssistant =
            serde_json::from_str(r#"{"enabled":true,"permissions":{"wear":"ask","later":"allow"},"other":1}"#).expect("parsed");
        assert!(s.enabled);
        assert_eq!(s.permissions.level(Class::Wear), Level::Ask);
        assert_eq!(s.permissions.level(Class::Read), Level::Allow);
        assert_eq!(s.permissions.level(Class::Trash), Level::Never);
        let json = serde_json::to_string(&s).expect("serialized");
        assert!(json.contains(r#""move":"never""#), "{json}");
    }
}
