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
    /// Folderstorm: walk or fly the avatar to a point, take off and land,
    /// teleport ("Move my avatar", phase 3, part 4). A teleport always asks.
    Navigation,
    /// Folderstorm: save a notecard's text, save and compile a script of
    /// the agent's own inventory (folderstorm/plans/mcp-content.md).
    Content,
    /// Folderstorm: put copies of items into objects the agent owns and may
    /// modify, rename or remove what they hold (removing always asks), and
    /// record an uncertain copy or rez (folderstorm/plans/mcp-objects.md).
    Objects,
    /// Folderstorm: take such objects, or copies of them, into the
    /// inventory (taking one out of the world always asks).
    Take,
    /// Folderstorm: start, stop and reset scripts inside such objects, and
    /// leave a script running after a save
    /// (folderstorm/plans/mcp-task-scripts.md).
    Scripts,
    /// Folderstorm: touch such objects, answer their script dialogs and
    /// speak on script channels near them
    /// (folderstorm/plans/mcp-script-testing.md).
    Interact,
    /// Folderstorm: move, turn, resize and repaint objects the user owns,
    /// and rez new ones on land where the user may build
    /// (folderstorm/plans/mcp-building.md).
    Build,
    /// Folderstorm: play gestures on the user's avatar (their animations,
    /// sounds and lines reach everyone nearby)
    /// (folderstorm/plans/mcp-gestures.md).
    Gestures,
    /// Folderstorm: pose the user's own avatar on this screen, and save
    /// that pose as a new file (folderstorm/plans/mcp-gestures.md).
    Pose,
}

impl Class {
    pub const ALL: [Class; 15] = [
        Class::Read,
        Class::Edit,
        Class::Move,
        Class::Wear,
        Class::Trash,
        Class::Camera,
        Class::Navigation,
        Class::Content,
        Class::Objects, // Folderstorm: mcp-objects.md
        Class::Take,
        Class::Scripts,  // Folderstorm: mcp-task-scripts.md
        Class::Interact, // Folderstorm: mcp-script-testing.md
        Class::Build,    // Folderstorm: mcp-building.md
        Class::Gestures, // Folderstorm: mcp-gestures.md
        Class::Pose,     // Folderstorm: mcp-gestures.md
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
/// other changes all ask ([`Saved`]), and so does a Navigation (or Content)
/// level missing from settings saved before it existed; so do the Objects
/// and Take levels missing from settings saved before them, the camera,
/// navigation and content included; and the Scripts level, objects and take
/// included; and the Interact level, Scripts included; and the Build
/// level, Interact included; and the Gestures level, Build included, then
/// the Pose level, Gestures included.
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
    pub navigation: Level,
    /// Folderstorm: content editing (mcp-content.md).
    pub content: Level,
    /// Folderstorm: object contents (mcp-objects.md).
    pub objects: Level,
    /// Folderstorm: taking objects (mcp-objects.md).
    pub take: Level,
    /// Folderstorm: running scripts inside objects (mcp-task-scripts.md).
    pub scripts: Level,
    /// Folderstorm: interacting with objects (mcp-script-testing.md).
    pub interact: Level,
    /// Folderstorm: building with owned objects (mcp-building.md).
    pub build: Level,
    /// Folderstorm: playing gestures (mcp-gestures.md).
    pub gestures: Level,
    /// Folderstorm: posing the user's own avatar (mcp-gestures.md).
    pub pose: Level,
}

/// [`Permissions`] as saved: Camera came with phase 3, Navigation and
/// Content after, Objects and Take later.
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
    navigation: Option<Level>,
    content: Option<Level>,
    objects: Option<Level>,
    take: Option<Level>,
    scripts: Option<Level>,
    interact: Option<Level>,
    build: Option<Level>,
    gestures: Option<Level>,
    pose: Option<Level>,
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
            navigation: None,
            content: None,
            objects: None,
            take: None,
            scripts: None,
            interact: None,
            build: None,
            gestures: None,
            pose: None,
        }
    }
}

impl From<Saved> for Permissions {
    fn from(s: Saved) -> Self {
        // "Ask before changes" chosen before the camera existed keeps that
        // preset: asking first is what it promised for every change
        let asked = [s.edit, s.move_, s.wear, s.trash].iter().all(|l| *l == Level::Ask);
        let camera = s.camera.unwrap_or(if asked { Level::Ask } else { Level::Never });
        // Folderstorm: the same for the navigation tools, the camera
        // included in "every change asks"
        let navigation = s
            .navigation
            .unwrap_or(if asked && camera == Level::Ask { Level::Ask } else { Level::Never });
        // Folderstorm: the same rule for content editing, the camera and
        // navigation included
        let asked = asked && camera == Level::Ask && navigation == Level::Ask;
        let content = s.content.unwrap_or(if asked { Level::Ask } else { Level::Never });
        // Folderstorm: the same for the object tools, the camera,
        // navigation and content included
        let later = if asked && content == Level::Ask { Level::Ask } else { Level::Never };
        let objects = s.objects.unwrap_or(later);
        let take = s.take.unwrap_or(later);
        // Folderstorm: the same for running scripts, objects and take included
        let scripts = s
            .scripts
            .unwrap_or(if later == Level::Ask && objects == Level::Ask && take == Level::Ask {
                Level::Ask
            } else {
                Level::Never
            });
        // Folderstorm: the same for interacting with objects, scripts included
        let interact = s.interact.unwrap_or(
            if later == Level::Ask && objects == Level::Ask && take == Level::Ask && scripts == Level::Ask {
                Level::Ask
            } else {
                Level::Never
            },
        );
        // Folderstorm: the same for building, interacting included
        let build = s.build.unwrap_or(
            if later == Level::Ask && objects == Level::Ask && take == Level::Ask && scripts == Level::Ask && interact == Level::Ask {
                Level::Ask
            } else {
                Level::Never
            },
        );
        // Folderstorm: the same for gestures, building included, then for
        // posing, gestures included
        let built = later == Level::Ask
            && objects == Level::Ask
            && take == Level::Ask
            && scripts == Level::Ask
            && interact == Level::Ask
            && build == Level::Ask;
        let gestures = s.gestures.unwrap_or(if built { Level::Ask } else { Level::Never });
        let pose = s.pose.unwrap_or(if built && gestures == Level::Ask {
            Level::Ask
        } else {
            Level::Never
        });
        Permissions {
            read: s.read,
            edit: s.edit,
            move_: s.move_,
            wear: s.wear,
            trash: s.trash,
            camera,
            navigation,
            content,
            objects,
            take,
            scripts,
            interact,
            build,
            gestures,
            pose,
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
            Class::Navigation => self.navigation,
            Class::Content => self.content,
            Class::Objects => self.objects,
            Class::Take => self.take,
            Class::Scripts => self.scripts,
            Class::Interact => self.interact,
            Class::Build => self.build,
            Class::Gestures => self.gestures,
            Class::Pose => self.pose,
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
            Class::Navigation => self.navigation = level,
            Class::Content => self.content = level,
            Class::Objects => self.objects = level,
            Class::Take => self.take = level,
            Class::Scripts => self.scripts = level,
            Class::Interact => self.interact = level,
            Class::Build => self.build = level,
            Class::Gestures => self.gestures = level,
            Class::Pose => self.pose = level,
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
            navigation: changes,
            content: changes,
            objects: changes,
            take: changes,
            scripts: changes,
            interact: changes,
            build: changes,
            gestures: changes,
            pose: changes,
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
            Class::Navigation,
            Class::Content,
            Class::Objects,
            Class::Take,
            Class::Scripts,
            Class::Interact,
            Class::Build,
            Class::Gestures,
            Class::Pose,
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

    /// Folderstorm: scripts inside objects (mcp-task-scripts.md).
    #[test]
    fn scripts_ask_in_ask_before_changes_and_keep_older_presets() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Scripts), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Scripts), Level::Never);
        assert!(Class::Scripts.can_ask());
        // "Ask before changes" saved before it, with or without the object classes
        for saved in [
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask"}"#,
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#,
        ] {
            let asked: Permissions = serde_json::from_str(saved).expect("parsed");
            assert_eq!(asked.level(Class::Scripts), Level::Ask, "{saved}");
            assert_eq!(asked.preset(), Preset::AskBeforeChanges, "{saved}");
        }
        // the objects allowed by hand: running scripts is not
        let custom: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"allow","take":"ask"}"#)
                .expect("parsed");
        assert_eq!(custom.level(Class::Scripts), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"scripts":"allow"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Scripts), Level::Allow);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""scripts":"allow""#), "{json}");
    }

    /// Folderstorm: script testing (mcp-script-testing.md).
    #[test]
    fn interact_asks_in_ask_before_changes_and_keeps_older_presets() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Interact), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Interact), Level::Never);
        assert!(Class::Interact.can_ask());
        // "Ask before changes" saved before it, with or without the later classes
        for saved in [
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask"}"#,
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#,
        ] {
            let asked: Permissions = serde_json::from_str(saved).expect("parsed");
            assert_eq!(asked.level(Class::Interact), Level::Ask, "{saved}");
            assert_eq!(asked.preset(), Preset::AskBeforeChanges, "{saved}");
        }
        // running scripts allowed by hand: interacting is not
        let custom: Permissions = serde_json::from_str(
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"allow"}"#,
        )
        .expect("parsed");
        assert_eq!(custom.level(Class::Interact), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"interact":"allow"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Interact), Level::Allow);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""interact":"allow""#), "{json}");
    }

    /// Folderstorm: building (mcp-building.md).
    #[test]
    fn build_asks_in_ask_before_changes_and_keeps_older_presets() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Build), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Build), Level::Never);
        assert!(Class::Build.can_ask());
        for saved in [
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask","interact":"ask"}"#,
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#,
        ] {
            let asked: Permissions = serde_json::from_str(saved).expect("parsed");
            assert_eq!(asked.level(Class::Build), Level::Ask, "{saved}");
            assert_eq!(asked.preset(), Preset::AskBeforeChanges, "{saved}");
        }
        // interacting allowed by hand: building is not
        let custom: Permissions = serde_json::from_str(
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask","interact":"allow"}"#,
        )
        .expect("parsed");
        assert_eq!(custom.level(Class::Build), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"build":"allow"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Build), Level::Allow);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""build":"allow""#), "{json}");
    }

    /// Folderstorm: gestures and posing (mcp-gestures.md).
    #[test]
    fn gestures_and_pose_ask_in_ask_before_changes_and_keep_older_presets() {
        for class in [Class::Gestures, Class::Pose] {
            assert_eq!(Preset::AskBeforeChanges.permissions().level(class), Level::Ask);
            assert_eq!(Preset::ReadOnly.permissions().level(class), Level::Never);
            assert!(class.can_ask());
        }
        for saved in [
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask","interact":"ask","build":"ask"}"#,
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#,
        ] {
            let asked: Permissions = serde_json::from_str(saved).expect("parsed");
            assert_eq!(asked.level(Class::Gestures), Level::Ask, "{saved}");
            assert_eq!(asked.level(Class::Pose), Level::Ask, "{saved}");
            assert_eq!(asked.preset(), Preset::AskBeforeChanges, "{saved}");
        }
        // building allowed by hand: neither is
        let custom: Permissions = serde_json::from_str(
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask","interact":"ask","build":"allow"}"#,
        )
        .expect("parsed");
        assert_eq!(custom.level(Class::Gestures), Level::Never);
        assert_eq!(custom.level(Class::Pose), Level::Never);
        // gestures saved as Never: posing does not ask either
        let no_gestures: Permissions = serde_json::from_str(
            r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask","objects":"ask","take":"ask","scripts":"ask","interact":"ask","build":"ask","gestures":"never"}"#,
        )
        .expect("parsed");
        assert_eq!(no_gestures.level(Class::Pose), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"gestures":"allow","pose":"ask"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Gestures), Level::Allow);
        assert_eq!(kept.level(Class::Pose), Level::Ask);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""gestures":"allow""#) && json.contains(r#""pose":"ask""#), "{json}");
    }

    #[test]
    fn navigation_asks_in_ask_before_changes_and_is_never_in_read_only() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Navigation), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Navigation), Level::Never);
        assert!(Class::Navigation.can_ask());
        // "Ask before changes" saved before the navigation tools keeps it
        let asked: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask"}"#).expect("parsed");
        assert_eq!(asked.level(Class::Navigation), Level::Ask);
        assert_eq!(asked.preset(), Preset::AskBeforeChanges);
        // the camera allowed by hand: Custom, and the avatar does not move
        let custom: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"allow"}"#).expect("parsed");
        assert_eq!(custom.level(Class::Navigation), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"navigation":"allow"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Navigation), Level::Allow);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""navigation":"allow""#), "{json}");
    }

    #[test]
    fn content_asks_in_ask_before_changes_and_settings_saved_before_it_keep_their_preset() {
        assert_eq!(Preset::AskBeforeChanges.permissions().level(Class::Content), Level::Ask);
        assert_eq!(Preset::ReadOnly.permissions().level(Class::Content), Level::Never);
        assert!(Class::Content.can_ask());
        // "Ask before changes" saved before content editing existed
        let asked: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"ask"}"#).expect("parsed");
        assert_eq!(asked.level(Class::Content), Level::Ask);
        assert_eq!(asked.preset(), Preset::AskBeforeChanges);
        // saved by phase 2 (no camera either): still that preset
        let older: Permissions = serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask"}"#).expect("parsed");
        assert_eq!(older.preset(), Preset::AskBeforeChanges);
        // the camera allowed or refused: content editing is not allowed
        let custom: Permissions =
            serde_json::from_str(r#"{"edit":"ask","move":"ask","wear":"ask","trash":"ask","camera":"allow"}"#).expect("parsed");
        assert_eq!(custom.level(Class::Content), Level::Never);
        let kept: Permissions = serde_json::from_str(r#"{"content":"allow"}"#).expect("parsed");
        assert_eq!(kept.level(Class::Content), Level::Allow);
        let json = serde_json::to_string(&kept).expect("serialized");
        assert!(json.contains(r#""content":"allow""#), "{json}");
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
