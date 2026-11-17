//! What a shortcut shows and whether it opens: its name, its icon, and why
//! it is greyed when it is (folderstorm/plans/margin-desktop.md,
//! "Shortcuts: what can be one"). Pure, but for the catalogue's labels.
//!
//! A folder shortcut keeps the folder's id and its account's: its name is
//! read from the inventory when it is drawn, and nothing of what the
//! folder holds is read here. Only a folder of the user's own inventory,
//! out of the Trash, can be one (the rule of the folder tabs,
//! `ui/inventory/custom_tabs/folders.rs` `status`).
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
use crate::world::inventory::{Inventory, actions as rules};
use aurora_i18n::tr;
use std::collections::BTreeMap;
use uuid::Uuid;

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
    "director",
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
    /// RLVa refuses the Build button (`ui::bars::rlva_build_locked_id`).
    pub build_locked: bool,
    /// The Fly button is held (RLVa, `ui::bars::fly_locked_id`).
    pub fly_locked: bool,
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
    /// The inventory is not known yet (the first moments of a session).
    NotYet,
    /// The folder was deleted, or is in the Trash.
    FolderGone,
    /// The folder is of another account of this profile.
    OtherAccount,
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
            Greyed::FolderGone => tr!("fs-desktop-greyed-folder-gone"),
            Greyed::OtherAccount => tr!("fs-desktop-greyed-other-account"),
        }
    }

    /// Whether a shortcut to it may be added: only what will never open
    /// from a shortcut is refused; what has nothing to do now may.
    pub fn refuses_adding(self) -> bool {
        matches!(self, Greyed::Gone | Greyed::NotReady | Greyed::ButtonOnly | Greyed::NotYet)
    }
}

/// Where a folder stands for a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderState {
    /// Of the user's own inventory, out of the Trash.
    Live,
    /// Not in the inventory, or in the Trash.
    Gone,
    /// The inventory's root, the Library or a folder of it: never a
    /// shortcut.
    Refused,
    /// The inventory is not known yet.
    Unknown,
}

/// Where the folder `id` stands in `inv`.
pub fn folder_state(inv: &Inventory, id: Uuid) -> FolderState {
    if inv.root.is_nil() {
        return FolderState::Unknown;
    }
    if id.is_nil() || id == inv.root || id == inv.lib_root {
        return FolderState::Refused;
    }
    if !inv.folders.contains_key(&id) {
        return FolderState::Gone;
    }
    let library = |id: Uuid| id == inv.lib_root || inv.folders.get(&id).is_some_and(|f| f.library);
    if rules::ancestors(inv, id).any(library) {
        return FolderState::Refused;
    }
    if rules::in_type(inv, id, 14) || !rules::under(inv, id, inv.root) {
        return FolderState::Gone;
    }
    FolderState::Live
}

/// Why nothing of a selection can be a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// RLVa hides the Inventory (`@showinv`).
    Rlva,
    /// No folder of the user's own inventory in it: items, the Library,
    /// the Trash.
    NoFolder,
}

impl Refusal {
    pub fn text(self) -> &'static str {
        match self {
            Refusal::Rlva => tr!("fs-desktop-greyed-rlva"),
            Refusal::NoFolder => tr!("fs-desktop-add-refused"),
        }
    }
}

/// What a selection of the Inventory gives the desktop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked {
    /// The folders that can be shortcuts, once each, in the selection's
    /// order. A link to a folder gives the folder.
    pub folders: Vec<Uuid>,
    /// The entries left out: items, folders of the Library or the Trash.
    pub left_out: usize,
}

/// A selection has at most this many entries looked at.
pub const MAX_PICKED: usize = super::store::MAX_SHORTCUTS;

/// The folders of `ids` (as the Inventory windows carry them, or select
/// them) that can be shortcuts.
pub fn pick(inv: &Inventory, ids: &[Uuid]) -> Picked {
    let mut out = Picked::default();
    for id in ids.iter().take(MAX_PICKED) {
        let folder = rules::original(inv, *id).filter(|f| inv.folders.contains_key(f));
        match folder {
            Some(folder) if folder_state(inv, folder) == FolderState::Live => {
                if !out.folders.contains(&folder) {
                    out.folders.push(folder);
                }
            }
            _ => out.left_out += 1,
        }
    }
    out.left_out += ids.len().saturating_sub(MAX_PICKED);
    out
}

/// What *Add to desktop* and a drop would add of `ids`, or why nothing.
/// `no_inventory`: RLVa hides the Inventory.
pub fn offer(inv: &Inventory, ids: &[Uuid], no_inventory: bool) -> Result<Picked, Refusal> {
    if no_inventory {
        return Err(Refusal::Rlva);
    }
    let picked = pick(inv, ids);
    if picked.folders.is_empty() {
        return Err(Refusal::NoFolder);
    }
    Ok(picked)
}

/// What the app knows of the Inventory this frame, for the folder
/// shortcuts.
#[derive(Clone, Copy)]
pub struct Folders<'a> {
    pub inv: &'a Inventory,
    /// The account logged in.
    pub account: Uuid,
    /// RLVa hides the Inventory (`@showinv`): a folder shortcut does not
    /// open, and says nothing of its folder.
    pub no_inventory: bool,
    /// The colours the user gave folders in the Inventory (Aurora's folder
    /// colours); None while they are turned off.
    pub colors: Option<&'a BTreeMap<Uuid, [u8; 3]>>,
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
        "build" if facts.build_locked => Err(Greyed::Rlva),
        "fly" if facts.fly_locked => Err(Greyed::Rlva),
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
    /// The folder it opens, when it is one that can be shown now: its path
    /// is the tooltip's, and *Show in Inventory* goes to it.
    pub folder: Option<Uuid>,
    /// The colour its icon takes (a folder the user coloured).
    pub tint: Option<[u8; 3]>,
}

impl Face {
    /// The tooltip: its own name in full when it has one (under the icon
    /// it may be cut), what it opens, and why it is greyed when it is.
    pub fn tip(&self) -> String {
        self.tip_of(&self.target_name)
    }

    /// The tooltip with `target` for what it opens (a folder's path).
    pub fn tip_of(&self, target: &str) -> String {
        let mut tip = String::new();
        if self.name != self.target_name {
            tip.push_str(&self.name);
            tip.push('\n');
        }
        tip.push_str(target);
        if let Some(why) = self.greyed {
            tip.push('\n');
            tip.push_str(why.text());
        }
        tip
    }
}

/// The icon of a command that is gone, and of a kind not opened yet.
const ICON_UNKNOWN: &str = "help-circle-filled";
/// The icon of a folder.
pub const ICON_FOLDER: &str = "folder-filled";
/// The icon of a folder shortcut while RLVa hides the Inventory.
pub const ICON_LOCKED: &str = "lock-filled";

/// The face of `shortcut` with what the app knows this frame.
pub fn face(shortcut: &Shortcut, facts: &Facts, folders: &Folders) -> Face {
    let (folder, account) = match &shortcut.target {
        Target::Command { command: id } => {
            let (target_name, icon, greyed) = match COMMANDS.iter().find(|c| c.id == id) {
                Some(command) => (command.label().to_owned(), command.icon, command_gate(id, facts).err()),
                None => (tr!("fs-desktop-command-gone").to_owned(), ICON_UNKNOWN, Some(Greyed::Gone)),
            };
            return Face {
                name: shortcut.name.clone().unwrap_or_else(|| target_name.clone()),
                target_name,
                icon: shortcut.icon.clone().unwrap_or_else(|| icon.to_owned()),
                greyed,
                folder: None,
                tint: None,
            };
        }
        Target::Folder { folder, account } => (*folder, *account),
    };
    // a folder that says nothing of itself: its own name is not read
    let unnamed = |icon: &str, why: Greyed, own_name: bool| {
        let target_name = tr!("fs-desktop-folder").to_owned();
        Face {
            name: shortcut.name.clone().filter(|_| own_name).unwrap_or_else(|| target_name.clone()),
            target_name,
            icon: icon.to_owned(),
            greyed: Some(why),
            folder: None,
            tint: None,
        }
    };
    if account != folders.account {
        return unnamed(ICON_FOLDER, Greyed::OtherAccount, true);
    }
    // (nor the name the user gave the shortcut: it is often the folder's)
    if folders.no_inventory {
        return unnamed(ICON_LOCKED, Greyed::Rlva, false);
    }
    match folder_state(folders.inv, folder) {
        FolderState::Unknown => unnamed(ICON_FOLDER, Greyed::NotYet, true),
        // (a folder moved under the Library cannot be: it is gone too)
        FolderState::Gone | FolderState::Refused => unnamed(ICON_FOLDER, Greyed::FolderGone, true),
        FolderState::Live => {
            let target_name = crate::ui::inventory::folder_name(folders.inv, folder);
            Face {
                name: shortcut.name.clone().unwrap_or_else(|| target_name.clone()),
                target_name,
                icon: shortcut.icon.clone().unwrap_or_else(|| ICON_FOLDER.to_owned()),
                greyed: None,
                folder: Some(folder),
                tint: folders.colors.and_then(|colors| colors.get(&folder)).copied(),
            }
        }
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
        // the row's gates, and the layouts' own
        let all = Facts {
            photo_roll_empty: true,
            no_guide: true,
            sit_locked: true,
            build_locked: true,
            fly_locked: true,
            toggle_unset: true,
        };
        assert_eq!(command_gate("photo_roll", &free), Ok(()));
        assert_eq!(command_gate("photo_roll", &all), Err(Greyed::NoPhoto));
        assert_eq!(command_gate("destinations", &free), Ok(()));
        assert_eq!(command_gate("destinations", &all), Err(Greyed::NoGuide));
        assert_eq!(command_gate("sit", &free), Ok(()));
        assert_eq!(command_gate("sit", &all), Err(Greyed::Rlva));
        // upstream's RLVa holds on the Build and Fly buttons
        assert_eq!(command_gate("build", &free), Ok(()));
        assert_eq!(command_gate("build", &all), Err(Greyed::Rlva));
        assert_eq!(command_gate("fly", &free), Ok(()));
        assert_eq!(command_gate("fly", &all), Err(Greyed::Rlva));
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
        let inv = Inventory::default();
        let folders = Folders {
            inv: &inv,
            account: Uuid::from_u128(1),
            no_inventory: false,
            colors: None,
        };
        let face_of = |s: &Shortcut| face(s, &Facts::default(), &folders);
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
        // a folder while the inventory is not known: greyed, unnamed
        shortcut.target = Target::Folder {
            folder: Uuid::from_u128(5),
            account: Uuid::from_u128(1),
        };
        shortcut.name = None;
        let folder = face_of(&shortcut);
        assert_eq!(folder.greyed, Some(Greyed::NotYet));
        assert!(!folder.name.contains('5'), "never an id as a name");
    }

    /// The demo inventory of the account 42, with a folder in its Trash
    /// (9001), a folder of the Library (9002) and a link to a folder (9003).
    fn inventory() -> Inventory {
        use crate::world::inventory::{FetchState, Folder};
        use aurora_net::inventory::{InvFolder, InvItem};
        let mut inv = Inventory {
            root: Uuid::from_u128(1),
            ..Default::default()
        };
        crate::world::inventory::demo::seed(&mut inv, Uuid::from_u128(42));
        let trash = rules::system(&inv, 14).expect("a Trash");
        for (id, parent, name, library) in [(9001, trash, "Old things", false), (9002, inv.lib_root, "Clothing", true)] {
            let id = Uuid::from_u128(id);
            inv.folders.insert(
                id,
                Folder {
                    info: InvFolder {
                        id,
                        parent,
                        name: name.into(),
                        type_default: -1,
                        version: 1,
                        ..Default::default()
                    },
                    children: Vec::new(),
                    items: Vec::new(),
                    state: FetchState::Fetched,
                    library,
                },
            );
        }
        inv.add_items(vec![
            InvItem {
                id: Uuid::from_u128(9003),
                parent: Uuid::from_u128(8000),
                name: "A link to the subfolder".into(),
                asset_type: 25,
                inv_type: 8,
                asset_id: Uuid::from_u128(8001),
                ..Default::default()
            },
            InvItem {
                id: Uuid::from_u128(9004),
                parent: Uuid::from_u128(8000),
                name: "A box".into(),
                asset_type: 6,
                inv_type: 6,
                asset_id: Uuid::from_u128(9999),
                ..Default::default()
            },
        ]);
        inv
    }

    const MINE: u128 = 8000;
    const SUB: u128 = 8001;

    #[test]
    fn only_a_live_folder_of_the_own_inventory_can_be_a_shortcut() {
        let inv = inventory();
        let state = |n: u128| folder_state(&inv, Uuid::from_u128(n));
        assert_eq!(state(MINE), FolderState::Live);
        assert_eq!(state(SUB), FolderState::Live);
        // in the Trash, the Trash itself, deleted
        assert_eq!(state(9001), FolderState::Gone);
        assert_eq!(folder_state(&inv, rules::system(&inv, 14).expect("a Trash")), FolderState::Gone);
        assert_eq!(state(123_456), FolderState::Gone);
        // the Library and what it holds, the inventory's root
        assert_eq!(state(9002), FolderState::Refused);
        assert_eq!(folder_state(&inv, inv.lib_root), FolderState::Refused);
        assert_eq!(folder_state(&inv, inv.root), FolderState::Refused);
        // nothing is known before the inventory is
        assert_eq!(folder_state(&Inventory::default(), Uuid::from_u128(MINE)), FolderState::Unknown);
    }

    #[test]
    fn a_selection_gives_its_folders_and_leaves_the_rest_out() {
        let inv = inventory();
        let ids = |list: &[u128]| list.iter().map(|n| Uuid::from_u128(*n)).collect::<Vec<_>>();
        // folders once each, in the selection's order; a link gives its folder
        let picked = pick(&inv, &ids(&[SUB, MINE, 9003, 9004, 9001, 9002, 123_456]));
        assert_eq!(picked.folders, ids(&[SUB, MINE]));
        assert_eq!(picked.left_out, 4, "an item, the Trash's, the Library's, an unknown id");
        assert_eq!(pick(&inv, &ids(&[9003])).folders, ids(&[SUB]));
        // nothing to add says why
        assert_eq!(offer(&inv, &ids(&[9004]), false), Err(Refusal::NoFolder));
        assert_eq!(offer(&inv, &ids(&[9001, 9002]), false), Err(Refusal::NoFolder));
        assert_eq!(offer(&inv, &[], false), Err(Refusal::NoFolder));
        assert_eq!(offer(&inv, &[inv.root], false), Err(Refusal::NoFolder));
        // @showinv: nothing is added
        assert_eq!(offer(&inv, &ids(&[MINE]), true), Err(Refusal::Rlva));
        assert_eq!(
            offer(&inv, &ids(&[MINE, 9004]), false).map(|p| (p.folders.len(), p.left_out)),
            Ok((1, 1))
        );
    }

    #[test]
    fn a_folder_shortcut_shows_its_folder_or_why_it_cannot() {
        let inv = inventory();
        let me = Uuid::from_u128(42);
        let colors = BTreeMap::from([(Uuid::from_u128(MINE), [1, 2, 3])]);
        let folders = Folders {
            inv: &inv,
            account: me,
            no_inventory: false,
            colors: Some(&colors),
        };
        let mut shortcut = Shortcut {
            id: 1,
            target: Target::Folder {
                folder: Uuid::from_u128(MINE),
                account: me,
            },
            slot: 0,
            col: 0,
            row: 0,
            name: None,
            icon: None,
        };
        let facts = Facts::default();
        // its name is the folder's, read now; its colour is the folder's
        let live = face(&shortcut, &facts, &folders);
        assert_eq!(live.name, rules::name(&inv, Uuid::from_u128(MINE)));
        assert_eq!((live.greyed, live.folder), (None, Some(Uuid::from_u128(MINE))));
        assert_eq!((live.icon.as_str(), live.tint), (ICON_FOLDER, Some([1, 2, 3])));
        // colours turned off in the Inventory
        let plain = face(&shortcut, &facts, &Folders { colors: None, ..folders });
        assert_eq!(plain.tint, None);
        // @showinv: a lock, no name, not even the one the user typed
        shortcut.name = Some("My secret folder".into());
        let locked = face(
            &shortcut,
            &facts,
            &Folders {
                no_inventory: true,
                ..folders
            },
        );
        assert_eq!((locked.icon.as_str(), locked.greyed), (ICON_LOCKED, Some(Greyed::Rlva)));
        assert_eq!(locked.name, tr!("fs-desktop-folder"));
        assert_eq!((locked.folder, locked.tint), (None, None));
        assert!(!locked.tip().contains("secret") && !locked.tip().contains(&live.target_name));
        // another account of the profile: greyed, the folder's name unread
        let other = face(
            &shortcut,
            &facts,
            &Folders {
                account: Uuid::from_u128(43),
                ..folders
            },
        );
        assert_eq!((other.greyed, other.folder), (Some(Greyed::OtherAccount), None));
        assert_eq!(other.target_name, tr!("fs-desktop-folder"));
        // in the Trash, or deleted: greyed, to be removed
        shortcut.name = None;
        for gone in [9001, 123_456] {
            shortcut.target = Target::Folder {
                folder: Uuid::from_u128(gone),
                account: me,
            };
            let face = face(&shortcut, &facts, &folders);
            assert_eq!((face.greyed, face.folder), (Some(Greyed::FolderGone), None), "{gone}");
            assert_eq!(face.name, tr!("fs-desktop-folder"));
            assert!(face.tip().contains(Greyed::FolderGone.text()));
        }
    }
}
