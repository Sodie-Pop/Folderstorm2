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
    /// Rename, describe, create folders.
    Edit,
    /// Move, copy, link.
    Move,
    /// Wear, take off, detach.
    Wear,
    /// Move to Trash, restore from it. Permanent delete and Empty
    /// Trash are never a class: always refused.
    Trash,
    /// Move the camera, take a picture (phase 3).
    Camera,
    /// Folderstorm: put copies of items into objects the agent owns and may
    /// modify, rename or remove what they hold (removing always asks), and
    /// record an uncertain copy or rez (folderstorm/plans/mcp-objects.md).
    Objects,
    /// Folderstorm: take such objects, or copies of them, into the
    /// inventory (taking one out of the world always asks).
    Take,
}

impl Class {
    pub const ALL: [Class; 8] = [
        Class::Read,
        Class::Edit,
        Class::Move,
        Class::Wear,
        Class::Trash,
        Class::Camera,
        Class::Objects, // Folderstorm: mcp-objects.md
        Class::Take,
    ];

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
/// of the field (Read: Allow, the others: Never); a Camera level missing
/// from settings saved before phase 3 follows "Ask before changes" when the
/// other changes all ask ([`Saved`]); so do the Objects and Take levels
/// missing from settings saved before them, the camera included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Saved")]
pub struct Permissions {
    pub read: Level,
    pub edit: Level,
    #[serde(rename = "move")]
    pub move_: Level,
    pub wear: Level,
    pub trash: Level,
    pub camera: Level,
    /// Folderstorm: object contents (mcp-objects.md).
    pub objects: Level,
    /// Folderstorm: taking objects (mcp-objects.md).
    pub take: Level,
}

/// [`Permissions`] as saved: Camera came with phase 3, Objects and Take
/// later.
#[derive(Deserialize)]
#[serde(default)]
struct Saved {
    read: Level,
    edit: Level,
    #[serde(rename = "move")]
    move_: Level,
    wear: Level,
    trash: Level,
    camera: Option<Level>,
    objects: Option<Level>,
    take: Option<Level>,
}

impl Default for Saved {
    fn default() -> Self {
        let p = Permissions::default();
        Saved {
            read: p.read,
            edit: p.edit,
            move_: p.move_,
            wear: p.wear,
            trash: p.trash,
            camera: None,
            objects: None,
            take: None,
        }
    }
}

impl From<Saved> for Permissions {
    fn from(s: Saved) -> Self {
        // "Ask before changes" chosen before the camera existed keeps that
        // preset: asking first is what it promised for every change
        let asked = [s.edit, s.move_, s.wear, s.trash].iter().all(|l| *l == Level::Ask);
        let camera = s.camera.unwrap_or(if asked { Level::Ask } else { Level::Never });
        // Folderstorm: the same for the object tools, the camera included
        let later = if asked && camera == Level::Ask { Level::Ask } else { Level::Never };
        Permissions {
            read: s.read,
            edit: s.edit,
            move_: s.move_,
            wear: s.wear,
            trash: s.trash,
            camera,
            objects: s.objects.unwrap_or(later),
            take: s.take.unwrap_or(later),
        }
    }
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
            Class::Camera => self.camera,
            Class::Objects => self.objects,
            Class::Take => self.take,
        };
        // Read has no Ask: a hand-edited file cannot give it one
        if level == Level::Ask && !class.can_ask() {
            Level::Never
        } else {
            level
        }
    }

    pub fn set(&mut self, class: Class, level: Level) {
        let level = if level == Level::Ask && !class.can_ask() {
            Level::Never
        } else {
            level
        };
        match class {
            Class::Read => self.read = level,
            Class::Edit => self.edit = level,
            Class::Move => self.move_ = level,
            Class::Wear => self.wear = level,
            Class::Trash => self.trash = level,
            Class::Camera => self.camera = level,
            Class::Objects => self.objects = level,
            Class::Take => self.take = level,
        }
    }

    /// The preset these levels match, Custom for any other combination.
    pub fn preset(&self) -> Preset {
        [Preset::ReadOnly, Preset::AskBeforeChanges]
            .into_iter()
            .find(|preset| Class::ALL.iter().all(|c| self.level(*c) == preset.permissions().level(*c)))
            .unwrap_or(Preset::Custom)
    }

    /// Whether a call of `class` may run without asking (health's "reads
    /// allowed").
    pub fn check(&self, class: Class) -> Result<(), Level> {
        match self.level(class) {
            Level::Allow => Ok(()),
            level => Err(level),
        }
    }

    /// What the bridge does with a call of `class`: run it, run it once the
    /// user allows it in the viewer's dialog, or refuse it.
    pub fn decide(&self, class: Class) -> Decision {
        match self.level(class) {
            Level::Allow => Decision::Run,
            Level::Ask => Decision::Ask,
            Level::Never => Decision::Refuse,
        }
    }

    /// The decision for a call that needs several classes (a thumbnail
    /// from a picture: Edit and Camera): any Never refuses, else any Ask
    /// asks once, else it runs.
    pub fn decide_all(&self, classes: impl IntoIterator<Item = Class>) -> Decision {
        classes
            .into_iter()
            .map(|c| self.decide(c))
            .fold(Decision::Run, |all, one| match (all, one) {
                (Decision::Refuse, _) | (_, Decision::Refuse) => Decision::Refuse,
                (Decision::Ask, _) | (_, Decision::Ask) => Decision::Ask,
                _ => Decision::Run,
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Run,
    /// The viewer asks the user first (60 s, refused on timeout).
    Ask,
    Refuse,
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
            camera: changes,
            objects: changes,
            take: changes,
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
        for class in [
            Class::Edit,
            Class::Move,
            Class::Wear,
            Class::Trash,
            Class::Camera,
            Class::Objects,
            Class::Take,
        ] {
            assert_eq!(d.permissions.check(class), Err(Level::Never));
        }
    }

    #[test]
    fn camera_asks_in_ask_before_changes_and_is_never_in_read_only() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Camera), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Camera), Level::Never);
        let mut p = Preset::AskBeforeChanges.permissions();
        p.set(Class::Camera, Level::Allow);
        assert_eq!(p.preset(), Preset::Custom);
        assert_eq!(p.decide(Class::Camera), Decision::Run);
    }

    #[test]
    fn settings_saved_before_the_camera_keep_their_preset() {
        // "Ask before changes" saved by phase 2
        let asked: Permissions =
            serde_json::from_str(r#"{"read":"allow","edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#).expect("parsed");
        assert_eq!(asked.level(Class::Camera), Level::Ask);
        assert_eq!(asked.preset(), Preset::AskBeforeChanges);
        // any other combination: the camera is not allowed
        let custom: Permissions = serde_json::from_str(r#"{"edit":"allow","move":"ask","wear":"ask","trash":"ask"}"#).expect("parsed");
        assert_eq!(custom.level(Class::Camera), Level::Never);
        let read_only: Permissions = serde_json::from_str("{}").expect("parsed");
        assert_eq!(read_only, Permissions::default());
        // a saved camera level is kept, and written back
        let kept: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"never"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Camera), Level::Never);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""camera":"never""#), "{json}");
    }

    /// Folderstorm: object inspection and lifecycle (mcp-objects.md).
    #[test]
    fn objects_and_take_ask_in_ask_before_changes_and_keep_older_presets() {
        for class in [Class::Objects, Class::Take] {
            assert_eq!(Preset::AskBeforeChanges.permissions().level(class), Level::Ask);
            assert_eq!(Preset::ReadOnly.permissions().level(class), Level::Never);
            assert!(class.can_ask());
        }
        // "Ask before changes" saved before them, with or without the camera
        let asked: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask"}"#).expect("parsed");
        assert_eq!(asked.level(Class::Objects), Level::Ask);
        assert_eq!(asked.level(Class::Take), Level::Ask);
        assert_eq!(asked.preset(), Preset::AskBeforeChanges);
        let older: Permissions = serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#).expect("parsed");
        assert_eq!(older.preset(), Preset::AskBeforeChanges);
        // the camera allowed: the object tools are not
        let custom: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"allow"}"#).expect("parsed");
        assert_eq!(custom.level(Class::Objects), Level::Never);
        assert_eq!(custom.level(Class::Take), Level::Never);
        // saved levels are kept and written back
        let kept: Permissions = serde_json::from_str(r#"{"objects":"allow","take":"ask"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Objects), Level::Allow);
        assert_eq!(kept.level(Class::Take), Level::Ask);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""objects":"allow""#) && json.contains(r#""take":"ask""#), "{json}");
    }

    #[test]
    fn several_classes_decide_together() {
        let mut p = Preset::ReadOnly.permissions();
        p.set(Class::Edit, Level::Allow);
        assert_eq!(p.decide_all([Class::Edit, Class::Camera]), Decision::Refuse, "Camera on Never");
        p.set(Class::Camera, Level::Ask);
        assert_eq!(p.decide_all([Class::Edit, Class::Camera]), Decision::Ask);
        p.set(Class::Camera, Level::Allow);
        assert_eq!(p.decide_all([Class::Edit, Class::Camera]), Decision::Run);
        p.set(Class::Edit, Level::Never);
        assert_eq!(p.decide_all([Class::Edit, Class::Camera]), Decision::Refuse, "Edit on Never");
        assert_eq!(p.decide_all([]), Decision::Run);
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
    fn each_level_decides_what_happens() {
        let p = Preset::AskBeforeChanges.permissions();
        assert_eq!(p.decide(Class::Read), Decision::Run);
        assert_eq!(p.decide(Class::Wear), Decision::Ask);
        assert_eq!(p.check(Class::Wear), Err(Level::Ask), "asking is not running at once");
        let mut p = Preset::ReadOnly.permissions();
        assert_eq!(p.decide(Class::Trash), Decision::Refuse);
        p.set(Class::Move, Level::Allow);
        assert_eq!(p.decide(Class::Move), Decision::Run);
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
