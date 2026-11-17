//! What a shortcut shows and whether it opens: its name, its icon, and why
//! it is greyed when it is (folderstorm/plans/margin-desktop.md,
//! "Shortcuts: what can be one"). Pure, but for the catalogue's labels.
//!
//! A command shortcut runs what its toolbar button runs
//! (`ui::bars::run_command`), and stops where the button stops: the gates
//! of the toolbar's row (`ui::bars`, the loop over a toolbar's buttons) are
//! not in `run_command`, so they are applied here. Each command of the
//! catalogue is classed below; a test compares the table with the
//! catalogue, so that a command upstream adds is looked at, with the gates
//! its button may have got, at the sync that brings it.

use super::store::{Shortcut, Target};
use crate::ui::toolbar::COMMANDS;
use aurora_i18n::tr;

/// How a command of the catalogue behaves as a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `run_command` does what the button does.
    Runs,
    /// The button is the control: a pop-up anchored to it (Favorites, VR
    /// view) or a key held on it (Speak). `run_command` does nothing.
    ButtonOnly,
}

/// The commands whose button is their control.
const BUTTON_ONLY: &[&str] = &["speak", "vr_view", "favorites"];

/// The commands `run_command` runs as the button does (those Aurora cannot
/// run yet are among them: the catalogue says which).
const RUNS: &[&str] = &[
    "conversations",
    "move",
    "view",
    "mouselook",
    "people",
    "friends",
    "inventory",
    "texture_library",
    "gestures",
    "appearance",
    "avatar_studio",
    "search",
    "places",
    "snapshot",
    "worldmap",
    "minimap",
    "performance",
    "sit",
    "ao",
    "movement",
    "preferences",
    "fly",
    "home",
    "aboutland",
    "build",
    "profile",
    "reporter",
    "regioninfo",
    "teleport_history",
    "landmark_here",
    "marketplacelistings",
    "myenvironments",
    "radar",
    "block_list",
    "contact_sets",
    "stop_animations",
    "snapshot_to_disk",
    "beacons",
    "immersive",
    "name_tags",
    "hide_interface",
    "huds",
    "photo_roll",
    "avatar",
    "destinations",
    "picks",
    "poser",
    "flickr",
    "primfeed",
    "RFO",
    "gridstatus",
    "voice",
    "quickprefs",
    "webbrowser",
    "defaultchatbar",
    "areasearch",
    "settings_debug",
    "statistics",
    "soundexplorer",
    "assetblacklist",
    "phototools",
    "phototools_view",
    "pose_stand",
    "conversation_log",
    "movelock",
    "water_walk",
    "resync_animations",
    "regiontracker",
    "group_titles",
    "wearable_favorites",
    "discord",
    "360capture",
    "omnifilter",
    "avatar_align",
    "face_nearest_avatar",
    "workspaces",
    "layout_toggle",
    "gesture_board",
];

/// The class of the command `id`; None for one this table does not know
/// (it is then never run from a shortcut).
pub fn class(id: &str) -> Option<Class> {
    if BUTTON_ONLY.contains(&id) {
        Some(Class::ButtonOnly)
    } else if RUNS.contains(&id) {
        Some(Class::Runs)
    } else {
        None
    }
}

/// What the app knows this frame that greys a toolbar button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The session has no photo: the Photo roll has nothing to show.
    pub photo_roll_empty: bool,
    /// The grid has no Destination Guide.
    pub no_guide: bool,
    /// RLVa would refuse the Sit / Stand button's action.
    pub sit_locked: bool,
    /// No layout is set for the two-layout toggle.
    pub toggle_unset: bool,
}

/// Why a shortcut does not open now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Greyed {
    /// The catalogue no longer has that command.
    Gone,
    /// Aurora cannot run it yet.
    NotReady,
    /// It only works from its toolbar button.
    ButtonOnly,
    /// The Photo roll is empty.
    NoPhoto,
    /// This grid has no Destination Guide.
    NoGuide,
    /// No layout to toggle to.
    ToggleUnset,
    /// RLVa refuses it now.
    Rlva,
    /// A kind of shortcut this version does not open (folders, until
    /// checkpoint 2 of the plan).
    NotYet,
}

impl Greyed {
    /// What the tooltip says about it.
    pub fn text(self) -> &'static str {
        match self {
            Greyed::Gone => tr!("fs-desktop-greyed-gone"),
            Greyed::NotReady | Greyed::NotYet => crate::ui::menu::not_yet(),
            Greyed::ButtonOnly => tr!("fs-desktop-greyed-button-only"),
            Greyed::NoPhoto => tr!("photo-reel-none"),
            Greyed::NoGuide => tr!("destinations-unavailable"),
            Greyed::ToggleUnset => tr!("fs-switcher-toggle-unset"),
            Greyed::Rlva => tr!("fs-desktop-greyed-rlva"),
        }
    }

    /// Whether a shortcut to it may be added: only what will never open
    /// from a shortcut is refused; what has nothing to do now may.
    pub fn refuses_adding(self) -> bool {
        matches!(self, Greyed::Gone | Greyed::NotReady | Greyed::ButtonOnly | Greyed::NotYet)
    }
}

/// Whether the command `id` runs from a shortcut now: its button's gates,
/// in the order the toolbar's row applies them.
pub fn command_gate(id: &str, facts: &Facts) -> Result<(), Greyed> {
    let Some(command) = COMMANDS.iter().find(|c| c.id == id) else {
        return Err(Greyed::Gone);
    };
    match class(id) {
        // (a command this build's table does not class is not run)
        None => return Err(Greyed::NotReady),
        Some(Class::ButtonOnly) => return Err(Greyed::ButtonOnly),
        Some(Class::Runs) => {}
    }
    if !command.ready {
        return Err(Greyed::NotReady);
    }
    match id {
        "photo_roll" if facts.photo_roll_empty => Err(Greyed::NoPhoto),
        "destinations" if facts.no_guide => Err(Greyed::NoGuide),
        "layout_toggle" if facts.toggle_unset => Err(Greyed::ToggleUnset),
        "sit" if facts.sit_locked => Err(Greyed::Rlva),
        _ => Ok(()),
    }
}

/// How a shortcut looks this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    /// Under the icon: the shortcut's own name, else the target's.
    pub name: String,
    /// The target's own name, for the tooltip.
    pub target_name: String,
    /// A Tabler icon, or one of the skin's aliases.
    pub icon: String,
    /// Why it does not open now.
    pub greyed: Option<Greyed>,
}

impl Face {
    /// The tooltip: its own name in full when it has one (under the icon
    /// it may be cut), what it opens, and why it is greyed when it is.
    pub fn tip(&self) -> String {
        let mut tip = String::new();
        if self.name != self.target_name {
            tip.push_str(&self.name);
            tip.push('\n');
        }
        tip.push_str(&self.target_name);
        if let Some(why) = self.greyed {
            tip.push('\n');
            tip.push_str(why.text());
        }
        tip
    }
}

/// The icon of a command that is gone, and of a kind not opened yet.
const ICON_UNKNOWN: &str = "help-circle-filled";
/// The icon of a folder (checkpoint 2 of the plan gives it its colour).
const ICON_FOLDER: &str = "folder-filled";

/// The face of `shortcut` with what the app knows this frame.
pub fn face(shortcut: &Shortcut, facts: &Facts) -> Face {
    let (target_name, icon, greyed) = match &shortcut.target {
        Target::Command { command: id } => match COMMANDS.iter().find(|c| c.id == id) {
            Some(command) => (command.label().to_owned(), command.icon, command_gate(id, facts).err()),
            None => (tr!("fs-desktop-command-gone").to_owned(), ICON_UNKNOWN, Some(Greyed::Gone)),
        },
        Target::Folder { .. } => (tr!("fs-desktop-folder").to_owned(), ICON_FOLDER, Some(Greyed::NotYet)),
    };
    Face {
        name: shortcut.name.clone().unwrap_or_else(|| target_name.clone()),
        target_name,
        icon: shortcut.icon.clone().unwrap_or_else(|| icon.to_owned()),
        greyed,
    }
}

/// The commands a desktop's *Add a command* lists: (id, label, icon, why
/// it cannot be added), sorted by their label in the interface's language.
pub fn addable() -> Vec<(&'static str, &'static str, &'static str, Option<Greyed>)> {
    let mut out: Vec<_> = COMMANDS
        .iter()
        .map(|c| {
            let refused = command_gate(c.id, &Facts::default()).err().filter(|why| why.refuses_adding());
            (c.id, c.label(), c.icon, refused)
        })
        .collect();
    out.sort_by_key(|(_, label, _, _)| label.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn every_command_of_the_catalogue_is_classed() {
        for command in COMMANDS {
            assert!(
                class(command.id).is_some(),
                "the toolbar command \"{}\" is new to the desktop: read its button's gates in ui/bars.rs (the loop over a \
                 toolbar's buttons) and class it in folderstorm/desktop/targets.rs",
                command.id
            );
        }
        // and the table names nothing the catalogue lost, nor twice
        for id in RUNS.iter().chain(BUTTON_ONLY) {
            assert!(COMMANDS.iter().any(|c| c.id == *id), "\"{id}\" is no longer in the catalogue");
        }
        assert_eq!(RUNS.len() + BUTTON_ONLY.len(), COMMANDS.len());
        assert!(RUNS.iter().all(|id| !BUTTON_ONLY.contains(id)));
    }

    #[test]
    fn a_command_stops_where_its_toolbar_button_stops() {
        let free = Facts::default();
        assert_eq!(command_gate("people", &free), Ok(()));
        // the button is the control
        for id in ["favorites", "vr_view", "speak"] {
            assert_eq!(command_gate(id, &free), Err(Greyed::ButtonOnly), "{id}");
        }
        // not in Aurora yet
        let unready = COMMANDS
            .iter()
            .find(|c| !c.ready && class(c.id) == Some(Class::Runs))
            .expect("one at least");
        assert_eq!(command_gate(unready.id, &free), Err(Greyed::NotReady));
        // gone from the catalogue
        assert_eq!(command_gate("no-such-command", &free), Err(Greyed::Gone));
        // the row's three gates, and the layouts' own
        let all = Facts {
            photo_roll_empty: true,
            no_guide: true,
            sit_locked: true,
            toggle_unset: true,
        };
        assert_eq!(command_gate("photo_roll", &free), Ok(()));
        assert_eq!(command_gate("photo_roll", &all), Err(Greyed::NoPhoto));
        assert_eq!(command_gate("destinations", &free), Ok(()));
        assert_eq!(command_gate("destinations", &all), Err(Greyed::NoGuide));
        assert_eq!(command_gate("sit", &free), Ok(()));
        assert_eq!(command_gate("sit", &all), Err(Greyed::Rlva));
        assert_eq!(command_gate("layout_toggle", &all), Err(Greyed::ToggleUnset));
        // they grey nothing else
        assert_eq!(command_gate("people", &all), Ok(()));
        assert_eq!(command_gate("home", &all), Ok(()));
    }

    #[test]
    fn what_can_be_added_and_how_a_shortcut_looks() {
        let list = addable();
        assert_eq!(list.len(), COMMANDS.len());
        let of = |id: &str| list.iter().find(|(c, ..)| *c == id).expect("listed").3;
        assert_eq!(of("people"), None);
        assert_eq!(of("favorites"), Some(Greyed::ButtonOnly));
        assert_eq!(of("vr_view"), Some(Greyed::ButtonOnly));
        // nothing to do now is no reason to refuse the shortcut
        assert_eq!(of("photo_roll"), None);
        assert!(!Greyed::Rlva.refuses_adding() && !Greyed::NoGuide.refuses_adding());
        let mut shortcut = Shortcut {
            id: 1,
            target: Target::command("people"),
            slot: 0,
            col: 0,
            row: 0,
            name: None,
            icon: None,
        };
        let face_of = |s: &Shortcut| face(s, &Facts::default());
        let people = face_of(&shortcut);
        assert_eq!(people.greyed, None);
        assert_eq!(people.icon, "Command_People_Icon");
        assert_eq!(people.name, people.target_name);
        assert_eq!(people.tip(), people.target_name);
        // a name and an icon of its own; the tooltip still says what it opens
        shortcut.name = Some("Friends list".into());
        shortcut.icon = Some("heart-filled".into());
        let own = face_of(&shortcut);
        assert_eq!((own.name.as_str(), own.icon.as_str()), ("Friends list", "heart-filled"));
        assert_eq!(own.target_name, people.target_name);
        assert_eq!(own.tip(), format!("Friends list\n{}", people.target_name));
        // a command the catalogue lost: greyed, with the reason
        shortcut.target = Target::command("retired");
        let gone = face_of(&shortcut);
        assert_eq!(gone.greyed, Some(Greyed::Gone));
        assert!(gone.tip().contains(Greyed::Gone.text()));
        // a folder is not opened by this version: kept, greyed, unnamed
        shortcut.target = Target::Folder {
            folder: Uuid::from_u128(5),
            account: Uuid::from_u128(1),
        };
        shortcut.name = None;
        let folder = face_of(&shortcut);
        assert_eq!(folder.greyed, Some(Greyed::NotYet));
        assert!(!folder.name.contains('5'), "never an id as a name");
    }
}
