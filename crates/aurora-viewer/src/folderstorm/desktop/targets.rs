//! What a shortcut shows and whether it opens: its name, its icon, and why
//! it is greyed when it is (folderstorm/plans/margin-desktop.md,
//! "Shortcuts: what can be one"). Pure, but for the catalogue's labels.
//!
//! A folder shortcut keeps the folder's id, in its account's desktop: its
//! name is read from the inventory when it is drawn, and nothing of what the
//! folder holds is read here. Only a folder of the user's own inventory,
//! out of the Trash, can be one (the rule of the folder tabs,
//! `ui/inventory/custom_tabs/folders.rs` `status`).
//!
//! An item shortcut keeps the item's id (a link followed when it was
//! added), never its asset id. What it is ([`ItemKind`]) is read from the
//! inventory when it is drawn, and it opens as the Inventory opens that
//! kind (`ui/inventory/context.rs` `open`, the bridge's `openItem` of the
//! Legacy Viewer), behind what greys that entry there. A landmark
//! teleports (LLLandmarkBridgeAction::doIt: TeleportFromLandmark asks
//! first), and one of the Library may be a shortcut, as the Legacy Viewer
//! teleports to it. A notecard opens in Aurora's notecard window through
//! `InvAction::Preview`, the Inventory's own *Open*: that path checks the
//! right to read it (LLPreviewNotecard::loadAsset: copy, or a god) and
//! RLVa's `@viewnote`; nothing of its content is read here.
//! Clothing and an object (an attachment, a HUD) are **added** to what is
//! worn, never replacing it: the Inventory's *Add*
//! (`Action::WearItems { replace: false }`), behind what greys that entry
//! (worn already, RLVa's wear locks: ERlvWearMask in LLWearableBridge and
//! LLObjectBridge::buildContextMenu; an item of a Marketplace listing). A
//! body part can only replace the one worn, so it is never a shortcut.
//!
//! An outfit shortcut keeps the id of an outfit folder of My Outfits.
//! Opening it wears the outfit as *Replace Current Outfit* of the
//! Inventory's menu does, behind what greys that entry there
//! (LLAppearanceMgr::getCanReplaceCOF, RLVa's folder locks), and always
//! through the question of outfit safety (FEATURES row 2.7,
//! `ui/inventory/outfit_safety.rs`).
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
use crate::world::appearance::FT_OUTFIT;
use crate::world::inventory::{Inventory, actions as rules};
use crate::world::rlva::WearLocks;
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
    /// RLVa refuses a teleport to a landmark now (`@tplm`, `@unsit` while
    /// seated: `World::rlva_can_teleport_landmark`, what greys *Teleport*
    /// on a landmark of the Inventory).
    pub landmark_blocked: bool,
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
    /// The item is not in the inventory: deleted, or not loaded yet.
    ItemGone,
    /// The item is in the Trash.
    ItemTrash,
    /// An item of a kind a shortcut does not open.
    ItemOther,
    /// The outfit's folder was deleted, is in the Trash, or is no longer
    /// an outfit.
    OutfitGone,
    /// What the outfit holds is not all loaded yet.
    OutfitLoading,
    /// The outfit holds nothing to wear.
    OutfitEmpty,
    /// An outfit change is under way.
    Busy,
    /// The clothing or the object is worn already.
    Worn,
    /// An item of a Marketplace listing is not worn.
    Listing,
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
            Greyed::ItemGone => tr!("fs-desktop-greyed-item-gone"),
            Greyed::ItemTrash => tr!("fs-desktop-greyed-item-trash"),
            Greyed::ItemOther => tr!("fs-desktop-greyed-item-other"),
            Greyed::OutfitGone => tr!("fs-desktop-greyed-outfit-gone"),
            Greyed::OutfitLoading => tr!("fs-desktop-greyed-outfit-loading"),
            Greyed::OutfitEmpty => tr!("fs-desktop-greyed-outfit-empty"),
            Greyed::Busy => tr!("fs-desktop-greyed-busy"),
            Greyed::Worn => tr!("fs-desktop-greyed-worn"),
            Greyed::Listing => tr!("fs-desktop-greyed-listing"),
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

/// What an item shortcut opens, by the item's asset type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// Opening it teleports there, after Aurora's question
    /// (TeleportFromLandmark).
    Landmark,
    /// Opening it opens Aurora's notecard window, as the Inventory's
    /// *Open* does.
    Notecard,
    /// Clothing or an object: opening it adds it to what is worn (the
    /// Inventory's *Add*), never replacing anything.
    Wearable,
}

impl ItemKind {
    /// The kind of an item of that asset type; None for one a shortcut
    /// does not open.
    pub fn of(asset_type: i32) -> Option<Self> {
        match asset_type {
            3 => Some(ItemKind::Landmark),
            7 => Some(ItemKind::Notecard),
            // (clothing, objects; a body part, 13, can only replace the
            // one worn: LLWearableBridge, "wear" alone for a body part)
            5 | 6 => Some(ItemKind::Wearable),
            _ => None,
        }
    }

    /// Whether an item of the Library can be a shortcut: what only reads
    /// it may (the Legacy Viewer teleports to a landmark of the Library,
    /// and opens a notecard of it when its rights allow). What is worn
    /// is not: wearing an item of the Library first copies it into the
    /// inventory, and Aurora's *Add* is greyed there.
    pub fn library(self) -> bool {
        match self {
            ItemKind::Landmark | ItemKind::Notecard => true,
            ItemKind::Wearable => false,
        }
    }

    /// What opening it is called in its menu.
    pub fn verb(self) -> &'static str {
        match self {
            ItemKind::Landmark => tr!("common-teleport"),
            ItemKind::Notecard => tr!("fs-desktop-open"),
            ItemKind::Wearable => tr!("ui-inventory-context-add-to-current-outfit"),
        }
    }
}

/// Where an item stands for a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// In the inventory, out of the Trash, of a kind a shortcut opens:
    /// `original` is the item itself, or the one its link points to.
    Live { original: Uuid, kind: ItemKind },
    /// Not in the inventory (deleted, or its folder is not loaded yet), or
    /// a broken link.
    Gone,
    /// In the Trash.
    Trash,
    /// Of a kind a shortcut does not open, or of the Library when that
    /// kind is not opened from there: never a shortcut.
    Refused,
    /// The inventory is not known yet.
    Unknown,
}

/// Where the item `id` stands in `inv`. A link is followed
/// (`actions::original`, as LLInvFVBridgeAction::doAction does).
pub fn item_state(inv: &Inventory, id: Uuid) -> ItemState {
    if inv.root.is_nil() {
        return ItemState::Unknown;
    }
    let Some(original) = rules::original(inv, id).and_then(|original| inv.items.get(&original)) else {
        return ItemState::Gone;
    };
    let Some(kind) = ItemKind::of(original.asset_type) else {
        return ItemState::Refused;
    };
    let live = ItemState::Live {
        original: original.id,
        kind,
    };
    if rules::library(inv, id) || rules::library(inv, original.id) {
        return if kind.library() { live } else { ItemState::Refused };
    }
    if rules::in_type(inv, id, 14) || rules::in_type(inv, original.id, 14) {
        return ItemState::Trash;
    }
    if !rules::under(inv, original.id, inv.root) {
        return ItemState::Gone;
    }
    live
}

/// The path of the item `id` in the Inventory, for its tooltip.
pub fn item_path(inv: &Inventory, id: Uuid) -> String {
    let Some(item) = inv.items.get(&id) else {
        return String::new();
    };
    let folder = crate::ui::inventory::inventory_path(inv, item.parent);
    if folder.is_empty() {
        item.name.clone()
    } else {
        format!("{folder} / {}", item.name)
    }
}

/// What opening an item shortcut does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemOpen {
    /// Teleport to this landmark, through Aurora's question
    /// (`PlacesUi::confirm_landmark`). The asset id only names the
    /// landmark in the teleport request, as `InvAction::TeleportLandmark`
    /// carries it: it is never shown nor kept.
    Teleport { asset: Uuid, name: String },
    /// Open this notecard (the item, a link followed) as the Inventory's
    /// *Open* does: `InvAction::Preview`, which checks the right to read
    /// it before anything is asked of the grid.
    Notecard(Uuid),
    /// Add this clothing or object (the item, a link followed) to what is
    /// worn, as the Inventory's *Add* does
    /// (`Action::WearItems { replace: false, point: 0 }`).
    Add(Uuid),
}

/// Whether the live item `original` of `kind` opens now: what greys its
/// entry in the Inventory's menu. `wear`: None reads as nothing worn and
/// nothing locked.
fn item_gate(inv: &Inventory, original: Uuid, kind: ItemKind, facts: &Facts, wear: Option<&Wearing>) -> Result<(), Greyed> {
    let Some(item) = inv.items.get(&original) else {
        return Err(Greyed::ItemGone);
    };
    match kind {
        // (a landmark without its asset yet is not one to teleport to;
        // RlvActions: @tplm, as the Inventory's Teleport entry)
        ItemKind::Landmark if item.asset_id.is_nil() => Err(Greyed::NotYet),
        ItemKind::Landmark if facts.landmark_blocked => Err(Greyed::Rlva),
        ItemKind::Landmark => Ok(()),
        // (RLVa's @viewnote: `Inventory::can_preview`, the first check of
        // the Inventory's own path)
        ItemKind::Notecard if !inv.can_preview(7) => Err(Greyed::Rlva),
        ItemKind::Notecard => Ok(()),
        // the Inventory's *Add* (`item_attachments`): not what is worn
        // already, not what an RLVa lock forbids adding; and nothing of a
        // Marketplace listing is worn (AlertMerchantListingCannotWear)
        ItemKind::Wearable => {
            if crate::world::marketplace::root(inv).is_some_and(|root| rules::under(inv, original, root)) {
                return Err(Greyed::Listing);
            }
            let Some(wear) = wear else {
                return Ok(());
            };
            if wear.worn.contains(&original) {
                Err(Greyed::Worn)
            } else if !wear.locks.wear(inv, original, 0).add {
                Err(Greyed::Rlva)
            } else {
                Ok(())
            }
        }
    }
}

/// [`item_open_worn`] with nothing worn and nothing locked.
#[cfg(test)]
pub fn item_open(inv: &Inventory, id: Uuid, facts: &Facts, no_inventory: bool) -> Result<ItemOpen, Greyed> {
    item_open_worn(inv, id, facts, None, no_inventory)
}

/// What opening the shortcut to the item `id` does now, or why it does
/// not open. `wear`: what is worn and what RLVa locks; `no_inventory`:
/// RLVa hides the Inventory (`@showinv`).
pub fn item_open_worn(inv: &Inventory, id: Uuid, facts: &Facts, wear: Option<&Wearing>, no_inventory: bool) -> Result<ItemOpen, Greyed> {
    if no_inventory {
        return Err(Greyed::Rlva);
    }
    let (original, kind) = match item_state(inv, id) {
        ItemState::Live { original, kind } => (original, kind),
        ItemState::Gone => return Err(Greyed::ItemGone),
        ItemState::Trash => return Err(Greyed::ItemTrash),
        ItemState::Refused => return Err(Greyed::ItemOther),
        ItemState::Unknown => return Err(Greyed::NotYet),
    };
    item_gate(inv, original, kind, facts, wear)?;
    // (said when it is opened, not drawn: it lasts a moment)
    if kind == ItemKind::Wearable && wear.is_some_and(|wear| wear.busy) {
        return Err(Greyed::Busy);
    }
    let item = inv.items.get(&original).ok_or(Greyed::ItemGone)?;
    Ok(match kind {
        ItemKind::Landmark => ItemOpen::Teleport {
            asset: item.asset_id,
            name: item.name.clone(),
        },
        ItemKind::Notecard => ItemOpen::Notecard(original),
        ItemKind::Wearable => ItemOpen::Add(original),
    })
}

/// Why nothing of a selection can be a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// RLVa hides the Inventory (`@showinv`).
    Rlva,
    /// Nothing in it that can be a shortcut: other kinds of items, the
    /// Trash, folders of the Library.
    Nothing,
}

impl Refusal {
    pub fn text(self) -> &'static str {
        match self {
            Refusal::Rlva => tr!("fs-desktop-greyed-rlva"),
            Refusal::Nothing => tr!("fs-desktop-add-refused"),
        }
    }
}

/// What a selection of the Inventory gives the desktop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked {
    /// What can be a shortcut, once each, in the selection's order: a
    /// folder, or an item of a kind a shortcut opens. A link gives what
    /// it points to.
    pub targets: Vec<Target>,
    /// The entries left out: other kinds of items, what is in the Trash,
    /// folders of the Library.
    pub left_out: usize,
}

impl Picked {
    /// The folders among them.
    #[cfg(test)]
    pub fn folders(&self) -> Vec<Uuid> {
        self.targets
            .iter()
            .filter_map(|target| match target {
                Target::Folder { folder } => Some(*folder),
                _ => None,
            })
            .collect()
    }

    /// The items among them.
    #[cfg(test)]
    pub fn items(&self) -> Vec<Uuid> {
        self.targets
            .iter()
            .filter_map(|target| match target {
                Target::Item { item } => Some(*item),
                _ => None,
            })
            .collect()
    }
}

/// A selection has at most this many entries looked at.
pub const MAX_PICKED: usize = super::store::MAX_SHORTCUTS;

/// Where the outfit folder `id` stands in `inv`: a live folder
/// ([`folder_state`]) that is an outfit (LLFolderType::FT_OUTFIT, as
/// `world::outfit_folders` tells one). A folder that is no longer an
/// outfit is refused.
pub fn outfit_state(inv: &Inventory, id: Uuid) -> FolderState {
    match folder_state(inv, id) {
        FolderState::Live if !inv.folders.get(&id).is_some_and(|f| f.info.type_default == FT_OUTFIT) => FolderState::Refused,
        state => state,
    }
}

/// What the app knows of what is worn this frame, for the outfit and the
/// wearable shortcuts.
#[derive(Clone, Default)]
pub struct Wearing {
    /// What RLVa's locks leave of wearing (`World::rlva_wear_locks`, what
    /// greys the Inventory's own entries).
    pub locks: WearLocks,
    /// The items worn now (`ui::appearance::items::worn_items`: what the
    /// Inventory marks "worn").
    pub worn: std::collections::HashSet<Uuid>,
    /// An outfit change is under way (`AppearanceUi::pending`).
    pub busy: bool,
}

/// Whether the outfit `id` can be worn now: what greys *Replace Current
/// Outfit* on its folder in the Inventory's menu
/// (`ui/inventory/context_catalog.rs`; LLAppearanceMgr::getCanReplaceCOF,
/// an RLVa add lock on the folder). Ok: ask first (outfit safety), then
/// wear. `wear`: None reads as nothing locked and nothing under way.
pub fn outfit_open(inv: &Inventory, id: Uuid, wear: Option<&Wearing>, no_inventory: bool) -> Result<(), Greyed> {
    if no_inventory {
        return Err(Greyed::Rlva);
    }
    match outfit_state(inv, id) {
        FolderState::Live => {}
        FolderState::Unknown => return Err(Greyed::NotYet),
        FolderState::Gone | FolderState::Refused => return Err(Greyed::OutfitGone),
    }
    if wear.is_some_and(|wear| !wear.locks.folder_allows(id, true)) {
        return Err(Greyed::Rlva);
    }
    if wear.is_some_and(|wear| wear.busy) {
        return Err(Greyed::Busy);
    }
    // (links still loading, or a folder not fetched yet: the app asks for
    // them, as the Inventory's menu does when it opens on that folder)
    match crate::world::appearance::wearable_contents(inv, id) {
        Err(_) => Err(Greyed::OutfitLoading),
        Ok(content) if content.is_empty() => Err(Greyed::OutfitEmpty),
        Ok(_) => Ok(()),
    }
}

/// What `id` gives the desktop: its folder (an outfit when it is one), or
/// its item when it is of a kind a shortcut opens. A link gives what it
/// points to.
fn target_of(inv: &Inventory, id: Uuid) -> Option<Target> {
    let original = rules::original(inv, id)?;
    if inv.folders.contains_key(&original) {
        if outfit_state(inv, original) == FolderState::Live {
            return Some(Target::Outfit { outfit: original });
        }
        return (folder_state(inv, original) == FolderState::Live).then_some(Target::Folder { folder: original });
    }
    match item_state(inv, id) {
        ItemState::Live { original, .. } => Some(Target::Item { item: original }),
        _ => None,
    }
}

/// The folders and the items of `ids` (as the Inventory windows carry
/// them, or select them) that can be shortcuts.
pub fn pick(inv: &Inventory, ids: &[Uuid]) -> Picked {
    let mut out = Picked::default();
    for id in ids.iter().take(MAX_PICKED) {
        match target_of(inv, *id) {
            Some(target) => {
                if !out.targets.contains(&target) {
                    out.targets.push(target);
                }
            }
            None => out.left_out += 1,
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
    if picked.targets.is_empty() {
        return Err(Refusal::Nothing);
    }
    Ok(picked)
}

/// What the app knows of the Inventory this frame, for the folder
/// shortcuts.
#[derive(Clone, Copy)]
pub struct Folders<'a> {
    pub inv: &'a Inventory,
    /// RLVa hides the Inventory (`@showinv`): a folder shortcut does not
    /// open, and says nothing of its folder.
    pub no_inventory: bool,
    /// The colours the user gave folders in the Inventory (Aurora's folder
    /// colours); None while they are turned off.
    pub colors: Option<&'a BTreeMap<Uuid, [u8; 3]>>,
    /// What is worn and what RLVa locks, for the outfit and the wearable
    /// shortcuts; None when the desktop has none (nothing is then greyed
    /// for it).
    pub wear: Option<&'a Wearing>,
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
    /// The folder it opens, when it is one that can be shown now: its path
    /// is the tooltip's, and *Show in Inventory* goes to it.
    pub folder: Option<Uuid>,
    /// The item it opens (the original, a link followed), when it is one
    /// that can be shown now: its path is the tooltip's, and *Show in
    /// Inventory* goes to it.
    pub item: Option<Uuid>,
    /// What opening it is called in its menu (*Teleport* for a landmark).
    pub verb: &'static str,
    /// RLVa hides the Inventory: its name and its icon are not its own,
    /// and none is given to it meanwhile.
    pub hidden: bool,
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
/// The icon of a folder or item shortcut while RLVa hides the Inventory.
pub const ICON_LOCKED: &str = "lock-filled";
/// The icon of an outfit.
pub const ICON_OUTFIT: &str = "hanger-2-filled";
/// The icon of an item that is gone (the Inventory's own for no type).
pub const ICON_ITEM: &str = "Inv_Invalid";

/// The face of `shortcut` with what the app knows this frame.
pub fn face(shortcut: &Shortcut, facts: &Facts, folders: &Folders) -> Face {
    let folder = match &shortcut.target {
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
                item: None,
                verb: tr!("fs-desktop-open"),
                hidden: false,
                tint: None,
            };
        }
        Target::Item { item } => return item_face(shortcut, *item, facts, folders),
        Target::Outfit { outfit } => return outfit_face(shortcut, *outfit, folders),
        Target::Folder { folder } => *folder,
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
            item: None,
            verb: tr!("fs-desktop-open"),
            hidden: !own_name,
            tint: None,
        }
    };
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
                item: None,
                verb: tr!("fs-desktop-open"),
                hidden: false,
                tint: folders.colors.and_then(|colors| colors.get(&folder)).copied(),
            }
        }
    }
}

/// The face of the shortcut to the outfit `id`: the folder's name, read
/// now, under a hanger; greyed with the reason when it is gone or when
/// RLVa locks its folder. What it holds is not walked here (it is when it
/// is opened): a shortcut is drawn every frame.
fn outfit_face(shortcut: &Shortcut, id: Uuid, folders: &Folders) -> Face {
    let unnamed = |icon: &str, why: Greyed, own_name: bool| {
        let target_name = if own_name {
            tr!("fs-desktop-outfit")
        } else {
            tr!("fs-desktop-folder")
        }
        .to_owned();
        Face {
            name: shortcut.name.clone().filter(|_| own_name).unwrap_or_else(|| target_name.clone()),
            target_name,
            icon: icon.to_owned(),
            greyed: Some(why),
            folder: None,
            item: None,
            verb: tr!("fs-desktop-wear-outfit"),
            hidden: !own_name,
            tint: None,
        }
    };
    // (@showinv: as a folder shortcut, nothing of it is said)
    if folders.no_inventory {
        return unnamed(ICON_LOCKED, Greyed::Rlva, false);
    }
    match outfit_state(folders.inv, id) {
        FolderState::Unknown => unnamed(ICON_OUTFIT, Greyed::NotYet, true),
        FolderState::Gone | FolderState::Refused => unnamed(ICON_OUTFIT, Greyed::OutfitGone, true),
        FolderState::Live => {
            let target_name = crate::ui::inventory::folder_name(folders.inv, id);
            let locked = folders.wear.is_some_and(|wear| !wear.locks.folder_allows(id, true));
            Face {
                name: shortcut.name.clone().unwrap_or_else(|| target_name.clone()),
                target_name,
                icon: shortcut.icon.clone().unwrap_or_else(|| ICON_OUTFIT.to_owned()),
                greyed: locked.then_some(Greyed::Rlva),
                folder: Some(id),
                item: None,
                verb: tr!("fs-desktop-wear-outfit"),
                hidden: false,
                tint: None,
            }
        }
    }
}

/// The face of the shortcut to the item `id`: the item's name and its
/// Inventory icon, read now; greyed with the reason when it is gone, in
/// the Trash, or when its entry of the Inventory's menu would be greyed.
fn item_face(shortcut: &Shortcut, id: Uuid, facts: &Facts, folders: &Folders) -> Face {
    // an item that says nothing of itself: its own name is not read
    let unnamed = |icon: &str, why: Greyed, own_name: bool| {
        let target_name = tr!("fs-desktop-item").to_owned();
        Face {
            name: shortcut.name.clone().filter(|_| own_name).unwrap_or_else(|| target_name.clone()),
            target_name,
            icon: icon.to_owned(),
            greyed: Some(why),
            folder: None,
            item: None,
            verb: tr!("fs-desktop-open"),
            hidden: !own_name,
            tint: None,
        }
    };
    // (RlvBehaviourToggleHandler<RLV_BHVR_SHOWINV>: as a folder shortcut,
    // neither its name nor the one the user gave it, often the same)
    if folders.no_inventory {
        return unnamed(ICON_LOCKED, Greyed::Rlva, false);
    }
    let inv = folders.inv;
    let (original, kind) = match item_state(inv, id) {
        ItemState::Live { original, kind } => (original, kind),
        ItemState::Unknown => return unnamed(ICON_ITEM, Greyed::NotYet, true),
        ItemState::Gone => return unnamed(ICON_ITEM, Greyed::ItemGone, true),
        ItemState::Trash => return unnamed(ICON_ITEM, Greyed::ItemTrash, true),
        ItemState::Refused => return unnamed(ICON_ITEM, Greyed::ItemOther, true),
    };
    let Some(item) = inv.items.get(&original) else {
        return unnamed(ICON_ITEM, Greyed::ItemGone, true);
    };
    let own = crate::ui::inventory::item_icon(item.asset_type, item.inv_type, item.flags);
    Face {
        name: shortcut.name.clone().unwrap_or_else(|| item.name.clone()),
        target_name: item.name.clone(),
        icon: shortcut.icon.clone().unwrap_or_else(|| own.to_owned()),
        greyed: item_gate(inv, original, kind, facts, folders.wear).err(),
        folder: None,
        item: Some(original),
        verb: kind.verb(),
        hidden: false,
        tint: None,
    }
}

/// Why a shortcut to the command `id` is never added (its button is its
/// control, Aurora cannot run it yet, the catalogue does not have it);
/// None when it may be, even if it has nothing to do now.
pub fn refuses_adding(id: &str) -> Option<Greyed> {
    command_gate(id, &Facts::default()).err().filter(|why| why.refuses_adding())
}

/// The commands a desktop's *Add a command* lists: (id, label, icon, why
/// it cannot be added), sorted by their label in the interface's language.
pub fn addable() -> Vec<(&'static str, &'static str, &'static str, Option<Greyed>)> {
    let mut out: Vec<_> = COMMANDS.iter().map(|c| (c.id, c.label(), c.icon, refuses_adding(c.id))).collect();
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
            landmark_blocked: true,
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
        let inv = Inventory::default();
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: None,
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
        };
        shortcut.name = None;
        let folder = face_of(&shortcut);
        assert_eq!(folder.greyed, Some(Greyed::NotYet));
        assert!(!folder.name.contains('5'), "never an id as a name");
    }

    /// The demo inventory of the account 42, with a folder in its Trash
    /// (9001), a folder of the Library (9002) and a link to a folder (9003);
    /// and items: a box (9004), a landmark (9100), a link to it (9101), a
    /// landmark in the Trash (9102), one of the Library (9103), one without
    /// its asset yet (9104), one the next owner got with no right at all
    /// (9106), a texture (9105); a notecard (9110), a link to it (9111), a
    /// notecard the agent may not read (9112, no copy) and one of the
    /// Library (9113).
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
        let item = |id: u128, parent: Uuid, name: &str, kind: i32, asset: u128| InvItem {
            id: Uuid::from_u128(id),
            parent,
            name: name.into(),
            asset_type: kind,
            inv_type: kind,
            asset_id: if asset == 0 { Uuid::nil() } else { Uuid::from_u128(asset) },
            owner: Uuid::from_u128(42),
            base_mask: rules::FULL,
            owner_mask: rules::FULL,
            ..Default::default()
        };
        let mine = Uuid::from_u128(8000);
        inv.add_items(vec![
            item(9100, mine, "Beach", 3, LANDMARK_ASSET),
            InvItem {
                asset_type: 24,
                ..item(9101, Uuid::from_u128(8001), "Beach", 3, 9100)
            },
            item(9102, Uuid::from_u128(9001), "Old place", 3, 9902),
            InvItem {
                owner: inv.lib_owner,
                everyone_mask: rules::COPY,
                ..item(9103, Uuid::from_u128(9002), "Welcome area", 3, 9903)
            },
            item(9104, mine, "Being made", 3, 0),
            item(9105, mine, "A picture", 0, 9905),
            // (no copy, no modify, no transfer)
            InvItem {
                owner_mask: 0,
                ..item(9106, mine, "Private club", 3, 9906)
            },
            item(9110, mine, "Rules", 7, NOTECARD_ASSET),
            InvItem {
                asset_type: 24,
                ..item(9111, Uuid::from_u128(8001), "Rules", 7, 9110)
            },
            InvItem {
                owner_mask: rules::MODIFY | rules::TRANSFER,
                ..item(9112, mine, "Sealed letter", 7, 9912)
            },
            InvItem {
                owner: inv.lib_owner,
                everyone_mask: rules::COPY,
                ..item(9113, Uuid::from_u128(9002), "Welcome note", 7, 9913)
            },
        ]);
        inv
    }

    /// The asset of the landmark 9100: never shown, never kept.
    const LANDMARK_ASSET: u128 = 0x00A5_5E70_0000_9900;

    /// The asset of the notecard 9110: never shown, never kept, never read here.
    const NOTECARD_ASSET: u128 = 0x00A5_5E70_0000_9910;

    #[test]
    fn a_notecard_shortcut_opens_through_the_inventory_s_own_path() {
        let mut inv = inventory();
        let free = Facts::default();
        let live = |n: u128| ItemState::Live {
            original: id(n),
            kind: ItemKind::Notecard,
        };
        assert_eq!(item_state(&inv, id(9110)), live(9110));
        assert_eq!(item_state(&inv, id(9111)), live(9110), "a link gives its notecard");
        assert_eq!(item_state(&inv, id(9113)), live(9113), "of the Library: read-only, by its rights");
        // what opens is the item, handed to `InvAction::Preview`: never
        // its asset, and no right is decided here
        assert_eq!(item_open(&inv, id(9110), &free, false), Ok(ItemOpen::Notecard(id(9110))));
        assert_eq!(item_open(&inv, id(9111), &free, false), Ok(ItemOpen::Notecard(id(9110))));
        assert_eq!(item_open(&inv, id(9113), &free, false), Ok(ItemOpen::Notecard(id(9113))));
        // one the agent may not read is still handed to that path, which
        // refuses it (LLPreviewNotecard::loadAsset: copy, or a god): the
        // shared rule says so, and nothing is read here to find out
        let sealed = inv.items.get(&id(9112)).expect("there");
        assert_eq!(rules::held(&inv, sealed) & rules::COPY, 0);
        assert_eq!(item_open(&inv, id(9112), &free, false), Ok(ItemOpen::Notecard(id(9112))));
        // a selection: once, by its item id
        let picked = pick(&inv, &[id(9111), id(9110), id(9112)]);
        assert_eq!(picked.items(), vec![id(9110), id(9112)]);
        let kept = serde_json::to_string(&picked.targets).expect("written");
        assert!(!kept.contains(&id(NOTECARD_ASSET).to_string()));
        // its face: the notecard's name and icon, *Open*
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: None,
        };
        let face_of = |n: u128, folders: &Folders| face(&item_shortcut(n), &free, folders);
        let rules_card = face_of(9110, &folders);
        assert_eq!((rules_card.name.as_str(), rules_card.icon.as_str()), ("Rules", "Inv_Notecard"));
        assert_eq!((rules_card.greyed, rules_card.verb), (None, tr!("fs-desktop-open")));
        // @showinv hides it; the landmark's gate does not grey it
        assert_eq!(item_open(&inv, id(9110), &free, true), Err(Greyed::Rlva));
        let blocked = Facts {
            landmark_blocked: true,
            ..Default::default()
        };
        assert_eq!(item_open(&inv, id(9110), &blocked, false), Ok(ItemOpen::Notecard(id(9110))));
        // RLVa's @viewnote: greyed, not opened; a landmark is untouched
        inv.rlva_preview_mask = 2;
        assert_eq!(item_open(&inv, id(9110), &free, false), Err(Greyed::Rlva));
        assert!(matches!(item_open(&inv, id(9100), &free, false), Ok(ItemOpen::Teleport { .. })));
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: None,
        };
        let greyed = face_of(9110, &folders);
        assert_eq!(
            (greyed.greyed, greyed.hidden, greyed.name.as_str()),
            (Some(Greyed::Rlva), false, "Rules")
        );
    }

    /// Wearables of the demo inventory (`world::appearance::seed_demo`): a
    /// shirt and an attachment that are not worn, a jacket of the current
    /// outfit, a body part.
    const SHIRT: u128 = 722;
    const ATTACHMENT: u128 = 723;
    const JACKET: u128 = 715;
    const SHAPE: u128 = 710;

    #[test]
    fn clothing_and_objects_are_added_never_replaced_and_a_body_part_is_no_shortcut() {
        use aurora_net::inventory::InvItem;
        let mut inv = inventory();
        let free = Facts::default();
        let live = |n: u128| ItemState::Live {
            original: id(n),
            kind: ItemKind::Wearable,
        };
        assert_eq!(item_state(&inv, id(SHIRT)), live(SHIRT));
        assert_eq!(item_state(&inv, id(ATTACHMENT)), live(ATTACHMENT));
        assert_eq!(item_state(&inv, id(9004)), live(9004), "an object");
        // a body part can only replace the one worn
        assert_eq!(item_state(&inv, id(SHAPE)), ItemState::Refused);
        assert_eq!(pick(&inv, &[id(SHAPE), id(SHIRT)]).targets, vec![Target::Item { item: id(SHIRT) }]);
        // opening adds: the item's id, to the Inventory's Add
        assert_eq!(item_open(&inv, id(SHIRT), &free, false), Ok(ItemOpen::Add(id(SHIRT))));
        assert_eq!(item_open(&inv, id(ATTACHMENT), &free, false), Ok(ItemOpen::Add(id(ATTACHMENT))));
        // through a link of an outfit to it
        let link = inv
            .items
            .values()
            .find(|it| it.asset_type == 24 && it.asset_id == id(JACKET))
            .map(|it| it.id)
            .expect("an outfit links the jacket");
        assert_eq!(item_state(&inv, link), live(JACKET));
        // worn already: greyed, nothing is added (nor taken off)
        let wearing = Wearing {
            worn: [id(JACKET)].into(),
            ..Default::default()
        };
        let open = |inv: &Inventory, n: u128, wear: &Wearing| item_open_worn(inv, id(n), &free, Some(wear), false);
        assert_eq!(open(&inv, JACKET, &wearing), Err(Greyed::Worn));
        assert_eq!(open(&inv, SHIRT, &wearing), Ok(ItemOpen::Add(id(SHIRT))));
        // an outfit change under way: said when opened
        let busy = Wearing {
            busy: true,
            ..Default::default()
        };
        assert_eq!(open(&inv, SHIRT, &busy), Err(Greyed::Busy));
        // @showinv
        assert_eq!(item_open(&inv, id(SHIRT), &free, true), Err(Greyed::Rlva));
        // its face: the Inventory's icon of its type, *Add to current outfit*
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: Some(&wearing),
        };
        let shirt = face(&item_shortcut(SHIRT), &free, &folders);
        let own = inv.items.get(&id(SHIRT)).expect("there");
        assert_eq!(shirt.icon, crate::ui::inventory::item_icon(own.asset_type, own.inv_type, own.flags));
        assert_eq!(
            (shirt.greyed, shirt.verb),
            (None, tr!("ui-inventory-context-add-to-current-outfit"))
        );
        let jacket = face(&item_shortcut(JACKET), &free, &folders);
        assert_eq!((jacket.greyed, jacket.item), (Some(Greyed::Worn), Some(id(JACKET))));
        assert!(jacket.tip().contains(Greyed::Worn.text()));
        // busy does not grey what is drawn
        let drawn_busy = Folders {
            wear: Some(&busy),
            ..folders
        };
        assert_eq!(face(&item_shortcut(SHIRT), &free, &drawn_busy).greyed, None);
        // of the Library: never (wearing it would first copy it)
        inv.add_items(vec![InvItem {
            id: id(9120),
            parent: id(9002),
            name: "Library shirt".into(),
            asset_type: 5,
            inv_type: 18,
            flags: 4,
            owner: inv.lib_owner,
            everyone_mask: rules::COPY,
            ..Default::default()
        }]);
        assert_eq!(item_state(&inv, id(9120)), ItemState::Refused);
        // in a Marketplace listing (the demo inventory's own folder): not worn
        let listings = crate::world::marketplace::root(&inv).expect("the demo's Marketplace listings");
        inv.add_items(vec![InvItem {
            id: id(9131),
            parent: listings,
            name: "Shirt for sale".into(),
            asset_type: 5,
            inv_type: 18,
            flags: 4,
            owner: id(42),
            owner_mask: rules::FULL,
            base_mask: rules::FULL,
            ..Default::default()
        }]);
        assert_eq!(item_open(&inv, id(9131), &free, false), Err(Greyed::Listing));
    }

    /// Outfits of the demo inventory (`world::appearance::seed_demo`): one
    /// that holds nothing, one with clothes and attachments.
    const EMPTY_OUTFIT: u128 = 702;
    const OUTFIT: u128 = 703;

    fn outfit_shortcut(n: u128) -> Shortcut {
        Shortcut {
            target: Target::Outfit { outfit: id(n) },
            ..item_shortcut(0)
        }
    }

    #[test]
    fn an_outfit_folder_gives_an_outfit_shortcut_and_wears_as_the_inventory_s_menu_allows() {
        let mut inv = inventory();
        assert_eq!(outfit_state(&inv, id(OUTFIT)), FolderState::Live);
        // a plain folder is not an outfit; gone is gone
        assert_eq!(outfit_state(&inv, id(MINE)), FolderState::Refused);
        assert_eq!(outfit_state(&inv, id(123_456)), FolderState::Gone);
        assert_eq!(outfit_state(&Inventory::default(), id(OUTFIT)), FolderState::Unknown);
        // a selection: an outfit folder is an outfit shortcut, another
        // folder a folder shortcut; the link to an outfit (as the current
        // outfit holds one) gives the outfit
        let link = inv
            .items
            .values()
            .find(|it| it.asset_type == 25 && outfit_state(&inv, it.asset_id) == FolderState::Live)
            .map(|it| (it.id, it.asset_id))
            .expect("the current outfit links its base outfit");
        let picked = pick(&inv, &[id(OUTFIT), id(MINE), link.0]);
        assert_eq!(
            picked.targets,
            vec![
                Target::Outfit { outfit: id(OUTFIT) },
                Target::Folder { folder: id(MINE) },
                Target::Outfit { outfit: link.1 }
            ]
        );
        // worn when Replace Current Outfit would be offered
        assert_eq!(outfit_open(&inv, id(OUTFIT), None, false), Ok(()));
        let free = Wearing::default();
        assert_eq!(outfit_open(&inv, id(OUTFIT), Some(&free), false), Ok(()));
        // nothing to wear, not an outfit, gone
        assert_eq!(outfit_open(&inv, id(EMPTY_OUTFIT), Some(&free), false), Err(Greyed::OutfitEmpty));
        assert_eq!(outfit_open(&inv, id(MINE), Some(&free), false), Err(Greyed::OutfitGone));
        assert_eq!(outfit_open(&inv, id(123_456), Some(&free), false), Err(Greyed::OutfitGone));
        assert_eq!(outfit_open(&Inventory::default(), id(OUTFIT), None, false), Err(Greyed::NotYet));
        // @showinv; an outfit change under way
        assert_eq!(outfit_open(&inv, id(OUTFIT), Some(&free), true), Err(Greyed::Rlva));
        let busy = Wearing {
            busy: true,
            ..Default::default()
        };
        assert_eq!(outfit_open(&inv, id(OUTFIT), Some(&busy), false), Err(Greyed::Busy));
        // its face: the folder's name under a hanger, and what opening does
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: Some(&free),
        };
        let facts = Facts::default();
        let mut shortcut = outfit_shortcut(OUTFIT);
        let live = face(&shortcut, &facts, &folders);
        assert_eq!(live.name, rules::name(&inv, id(OUTFIT)));
        assert_eq!(
            (live.icon.as_str(), live.greyed, live.folder),
            (ICON_OUTFIT, None, Some(id(OUTFIT)))
        );
        assert_eq!(live.verb, tr!("fs-desktop-wear-outfit"));
        // @showinv: locked and unnamed, as a folder
        shortcut.name = Some("Work clothes".into());
        let locked = face(
            &shortcut,
            &facts,
            &Folders {
                no_inventory: true,
                ..folders
            },
        );
        assert_eq!(
            (locked.icon.as_str(), locked.greyed, locked.hidden),
            (ICON_LOCKED, Some(Greyed::Rlva), true)
        );
        assert!(!locked.tip().contains("Work clothes") && !locked.tip().contains(&live.target_name));
        // gone: greyed, its own name kept
        shortcut.target = Target::Outfit { outfit: id(123_456) };
        let gone = face(&shortcut, &facts, &folders);
        assert_eq!(
            (gone.greyed, gone.folder, gone.name.as_str()),
            (Some(Greyed::OutfitGone), None, "Work clothes")
        );
        // what it holds is not loaded yet: asked for, not worn
        if let Some(folder) = inv.folders.get_mut(&id(OUTFIT)) {
            folder.state = crate::world::inventory::FetchState::Unknown;
        }
        assert_eq!(outfit_open(&inv, id(OUTFIT), None, false), Err(Greyed::OutfitLoading));
    }

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn item_shortcut(n: u128) -> Shortcut {
        Shortcut {
            id: 1,
            target: Target::Item { item: id(n) },
            slot: 0,
            col: 0,
            row: 0,
            name: None,
            icon: None,
        }
    }

    #[test]
    fn a_landmark_can_be_a_shortcut_where_the_inventory_teleports_to_it() {
        let inv = inventory();
        let live = |n: u128| ItemState::Live {
            original: id(n),
            kind: ItemKind::Landmark,
        };
        assert_eq!(item_state(&inv, id(9100)), live(9100));
        // a link gives what it points to (LLInvFVBridgeAction::doAction)
        assert_eq!(item_state(&inv, id(9101)), live(9100));
        // the rights held change nothing to a teleport (the Legacy Viewer
        // asks for none): full, or none at all
        assert_eq!(item_state(&inv, id(9106)), live(9106));
        // of the Library: the Legacy Viewer teleports to it
        assert_eq!(item_state(&inv, id(9103)), live(9103));
        // in the Trash, deleted (or not loaded yet), another kind of item
        assert_eq!(item_state(&inv, id(9102)), ItemState::Trash);
        assert_eq!(item_state(&inv, id(123_456)), ItemState::Gone);
        assert_eq!(item_state(&inv, id(9105)), ItemState::Refused);
        // nothing is known before the inventory is
        assert_eq!(item_state(&Inventory::default(), id(9100)), ItemState::Unknown);
        // a selection: the link and its landmark are one shortcut, the
        // item's id is kept, the rest is counted
        let picked = pick(&inv, &[id(9101), id(9100), id(MINE), id(9105), id(9102)]);
        assert_eq!(
            picked.targets,
            vec![Target::Item { item: id(9100) }, Target::Folder { folder: id(MINE) }]
        );
        assert_eq!((picked.items(), picked.left_out), (vec![id(9100)], 2));
        assert_eq!(offer(&inv, &[id(9105)], false), Err(Refusal::Nothing));
        assert_eq!(offer(&inv, &[id(9100)], true), Err(Refusal::Rlva));
        // what is kept is the item's id: never its asset's
        let kept = serde_json::to_string(&picked.targets).expect("written");
        assert!(kept.contains(&id(9100).to_string()));
        assert!(!kept.contains(&id(LANDMARK_ASSET).to_string()));
    }

    #[test]
    fn a_landmark_shortcut_teleports_unless_rlva_refuses_it() {
        let inv = inventory();
        let free = Facts::default();
        let teleport = Ok(ItemOpen::Teleport {
            asset: id(LANDMARK_ASSET),
            name: "Beach".into(),
        });
        assert_eq!(item_open(&inv, id(9100), &free, false), teleport);
        assert_eq!(item_open(&inv, id(9101), &free, false), teleport, "through its link");
        assert!(matches!(item_open(&inv, id(9103), &free, false), Ok(ItemOpen::Teleport { .. })));
        // @tplm (what greys Teleport on a landmark of the Inventory), @showinv
        let blocked = Facts {
            landmark_blocked: true,
            ..Default::default()
        };
        assert_eq!(item_open(&inv, id(9100), &blocked, false), Err(Greyed::Rlva));
        assert_eq!(item_open(&inv, id(9100), &free, true), Err(Greyed::Rlva));
        // what cannot open says why
        assert_eq!(item_open(&inv, id(9102), &free, false), Err(Greyed::ItemTrash));
        assert_eq!(item_open(&inv, id(123_456), &free, false), Err(Greyed::ItemGone));
        assert_eq!(item_open(&inv, id(9105), &free, false), Err(Greyed::ItemOther));
        assert_eq!(item_open(&inv, id(9104), &free, false), Err(Greyed::NotYet));
        assert_eq!(item_open(&Inventory::default(), id(9100), &free, false), Err(Greyed::NotYet));
    }

    #[test]
    fn an_item_shortcut_shows_its_item_or_why_it_cannot() {
        let inv = inventory();
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: None,
            wear: None,
        };
        let facts = Facts::default();
        let mut shortcut = item_shortcut(9101);
        // its name and its icon are the item's (the link's target), read now
        let live = face(&shortcut, &facts, &folders);
        assert_eq!((live.name.as_str(), live.icon.as_str()), ("Beach", "Inv_Landmark"));
        assert_eq!((live.greyed, live.item, live.folder), (None, Some(id(9100)), None));
        assert_eq!(live.verb, tr!("common-teleport"));
        assert!(!live.hidden);
        // its tooltip is its path in the Inventory; no id is ever said
        let path = item_path(&inv, id(9100));
        assert!(path.ends_with(" / Beach") && path.contains(&rules::name(&inv, id(MINE))), "{path}");
        for text in [live.tip_of(&path), live.name.clone(), live.icon.clone()] {
            assert!(!text.contains(&id(LANDMARK_ASSET).to_string()) && !text.contains(&id(9100).to_string()));
        }
        // a name and an icon of its own
        shortcut.name = Some("Holidays".into());
        shortcut.icon = Some("star-filled".into());
        let own = face(&shortcut, &facts, &folders);
        assert_eq!((own.name.as_str(), own.icon.as_str()), ("Holidays", "star-filled"));
        assert_eq!(own.tip(), "Holidays\nBeach");
        // RLVa refuses the teleport: greyed, its name stays
        let blocked = Facts {
            landmark_blocked: true,
            ..Default::default()
        };
        let greyed = face(&shortcut, &blocked, &folders);
        assert_eq!(
            (greyed.greyed, greyed.hidden, greyed.item),
            (Some(Greyed::Rlva), false, Some(id(9100)))
        );
        assert_eq!(greyed.name, "Holidays");
        // @showinv: a lock, no name, not even the one the user typed
        let locked = face(
            &shortcut,
            &facts,
            &Folders {
                no_inventory: true,
                ..folders
            },
        );
        assert_eq!((locked.icon.as_str(), locked.greyed), (ICON_LOCKED, Some(Greyed::Rlva)));
        assert_eq!(locked.name, tr!("fs-desktop-item"));
        assert!(locked.hidden && locked.item.is_none());
        assert!(!locked.tip().contains("Holidays") && !locked.tip().contains("Beach"));
        // gone, in the Trash, of another kind: greyed, unnamed, with the reason
        shortcut.name = None;
        shortcut.icon = None;
        for (n, why) in [
            (123_456, Greyed::ItemGone),
            (9102, Greyed::ItemTrash),
            (9105, Greyed::ItemOther),
            (9104, Greyed::NotYet),
        ] {
            shortcut.target = Target::Item { item: id(n) };
            let face = face(&shortcut, &facts, &folders);
            assert_eq!(face.greyed, Some(why), "{n}");
            if why != Greyed::NotYet {
                assert_eq!((face.name.as_str(), face.item), (tr!("fs-desktop-item"), None), "{n}");
                assert_eq!(face.icon, ICON_ITEM);
            }
            assert!(face.tip().contains(why.text()));
        }
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
        let picked = pick(&inv, &ids(&[SUB, MINE, 9003, 9105, 9001, 9002, 123_456]));
        assert_eq!(picked.folders(), ids(&[SUB, MINE]));
        assert_eq!(picked.left_out, 4, "an item, the Trash's, the Library's, an unknown id");
        assert_eq!(pick(&inv, &ids(&[9003])).folders(), ids(&[SUB]));
        // nothing to add says why
        assert_eq!(offer(&inv, &ids(&[9105]), false), Err(Refusal::Nothing));
        assert_eq!(offer(&inv, &ids(&[9001, 9002]), false), Err(Refusal::Nothing));
        assert_eq!(offer(&inv, &[], false), Err(Refusal::Nothing));
        assert_eq!(offer(&inv, &[inv.root], false), Err(Refusal::Nothing));
        // @showinv: nothing is added
        assert_eq!(offer(&inv, &ids(&[MINE]), true), Err(Refusal::Rlva));
        assert_eq!(
            offer(&inv, &ids(&[MINE, 9105]), false).map(|p| (p.targets.len(), p.left_out)),
            Ok((1, 1))
        );
    }

    #[test]
    fn a_folder_shortcut_shows_its_folder_or_why_it_cannot() {
        let inv = inventory();
        let colors = BTreeMap::from([(Uuid::from_u128(MINE), [1, 2, 3])]);
        let folders = Folders {
            inv: &inv,
            no_inventory: false,
            colors: Some(&colors),
            wear: None,
        };
        let mut shortcut = Shortcut {
            id: 1,
            target: Target::Folder {
                folder: Uuid::from_u128(MINE),
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
        // in the Trash, or deleted: greyed, to be removed
        shortcut.name = None;
        for gone in [9001, 123_456] {
            shortcut.target = Target::Folder {
                folder: Uuid::from_u128(gone),
            };
            let face = face(&shortcut, &facts, &folders);
            assert_eq!((face.greyed, face.folder), (Some(Greyed::FolderGone), None), "{gone}");
            assert_eq!(face.name, tr!("fs-desktop-folder"));
            assert!(face.tip().contains(Greyed::FolderGone.text()));
        }
    }
}
