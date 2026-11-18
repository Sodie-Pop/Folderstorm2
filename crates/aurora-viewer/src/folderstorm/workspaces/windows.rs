//! The windows a layout may place: an allowlist, in code
//! (folderstorm/plans/workspaces.md section 7.3). A window that is not
//! listed here is never captured and never moved: Preferences, dialogs and
//! confirmations, per-object windows (profiles, item properties, script and
//! notecard editors), Inventory folder windows, torn-off conversations,
//! notifications. An id found in a saved file and not here is ignored.
//!
//! Where a window's module has a `WINDOW` constant it is used, so a rename
//! upstream fails to compile at the merge. The other ids are the strings
//! their windows are made with; a test reads them back from those files.

use aurora_i18n::tr;
use std::borrow::Cow;

/// How a window is drawn, so how it is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Aurora's `Floater`: placed with `place_floater`, may be minimized.
    Floater,
    /// A window without a title bar (`bare_window`, the mini-map): placed
    /// through our own request.
    Bare,
}

/// One window of the allowlist.
pub struct Window {
    /// The id its window is made with (`egui::Id::unique(id)`).
    pub id: &'static str,
    pub kind: Kind,
    label: Label,
}

/// How a window's name is found.
enum Label {
    Text(fn() -> &'static str),
    /// An additional Inventory window, by its slot: "Inventory 2" …
    InventorySlot(u8),
}

impl Window {
    /// Its name, as its title bar or its menu entry has it.
    pub fn label(&self) -> Cow<'static, str> {
        match self.label {
            Label::Text(label) => Cow::Borrowed(label()),
            Label::InventorySlot(slot) => Cow::Owned(crate::ui::inventory::extra_windows::slot_title(slot)),
        }
    }
}

/// The first additional Inventory window (`ui/inventory.rs`,
/// `ADDITIONAL_WINDOW` through `floater_id`): Aurora's one window, with a
/// fixed id.
pub const ADDITIONAL_INVENTORY: &str = "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901090";

/// The additional Inventory windows, slots 1 to 16
/// (`ui/inventory/extra_windows.rs`, `id_of` through `floater_id`): the
/// ids that follow the first one's.
pub const INVENTORY_SLOTS: [&str; 16] = [
    ADDITIONAL_INVENTORY,
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901091",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901092",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901093",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901094",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901095",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901096",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901097",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901098",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901099",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109a",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109b",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109c",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109d",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109e",
    "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a90109f",
];

const fn floater(id: &'static str, label: fn() -> &'static str) -> Window {
    Window {
        id,
        kind: Kind::Floater,
        label: Label::Text(label),
    }
}

/// The additional Inventory window of slot `slot` (1 to 16).
const fn inventory_slot(slot: u8) -> Window {
    Window {
        id: INVENTORY_SLOTS[slot as usize - 1],
        kind: Kind::Floater,
        label: Label::InventorySlot(slot),
    }
}

static WINDOWS: &[Window] = &[
    floater("inventory", || tr!("common-inventory")),
    inventory_slot(1),
    inventory_slot(2),
    inventory_slot(3),
    inventory_slot(4),
    inventory_slot(5),
    inventory_slot(6),
    inventory_slot(7),
    inventory_slot(8),
    inventory_slot(9),
    inventory_slot(10),
    inventory_slot(11),
    inventory_slot(12),
    inventory_slot(13),
    inventory_slot(14),
    inventory_slot(15),
    inventory_slot(16),
    // geometry only: never its conversations or what they say
    floater("conversations", || tr!("common-conversations")),
    floater(crate::ui::people::WINDOW, || tr!("common-people")),
    floater("contacts", || tr!("common-contacts")),
    floater("places", || tr!("common-places")),
    floater(crate::ui::search::WINDOW, || tr!("search-title")),
    Window {
        id: "minimap",
        kind: Kind::Bare,
        label: Label::Text(|| tr!("common-mini-map")),
    },
    floater("worldmap", || tr!("common-world-map")),
    floater("perf", || tr!("common-performance")),
    floater(crate::ui::controls::WINDOW, || tr!("controls-title")),
    floater(crate::ui::controls::CLASSIC_CAMERA, || tr!("controls-camera-title")),
    floater(crate::ui::controls::CLASSIC_MOVEMENT, || tr!("controls-tab-movement")),
    floater("gestures", || tr!("gesture-title")),
    floater("appearance", || tr!("common-appearance")),
    floater("ao", || tr!("ao-title")),
    floater(crate::ui::region_tracker::WINDOW, || tr!("toolbar-command-region-tracker")),
    floater(crate::ui::area_search::WINDOW, || tr!("toolbar-command-area-search")),
    floater(crate::ui::quick_prefs::WINDOW, || tr!("toolbar-command-quickprefs")),
    floater(crate::ui::destinations::WINDOW, || tr!("toolbar-command-destinations")),
    floater(crate::ui::money_tracker::WINDOW, || tr!("money-tracker-title")),
];

/// Every window a layout may place.
pub fn all() -> &'static [Window] {
    WINDOWS
}

/// The window of that id, if a layout may place it.
pub fn find(id: &str) -> Option<&'static Window> {
    WINDOWS.iter().find(|window| window.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_within_the_store_s_limit() {
        let mut seen = std::collections::BTreeSet::new();
        for window in all() {
            assert!(seen.insert(window.id), "{} twice", window.id);
            assert!(!window.label().is_empty(), "{}", window.id);
        }
        assert!(all().len() <= super::super::store::MAX_WINDOWS);
    }

    #[test]
    fn what_is_never_placed_is_not_on_the_list() {
        for id in [
            crate::ui::options::WINDOW,
            "inventory_00000000-0000-0000-0000-000000000000",
            "chat_history",
            "bug_report",
            "",
        ] {
            assert!(find(id).is_none(), "{id}");
        }
        assert!(find("inventory").is_some_and(|w| w.kind == Kind::Floater));
        assert!(find("minimap").is_some_and(|w| w.kind == Kind::Bare));
    }

    /// The ids written here as strings are still the ones their windows are
    /// made with: an upstream rename fails this test at the merge.
    #[test]
    fn the_string_ids_are_the_ones_the_windows_use() {
        // (one line each: a checkout may change the line ends)
        let made_with = [
            ("inventory", include_str!("../../ui/inventory.rs"), "        \"inventory\","),
            (
                "conversations",
                include_str!("../../ui/chat.rs"),
                "Floater::new(\"conversations\", title",
            ),
            ("contacts", include_str!("../../ui/contacts.rs"), "        \"contacts\","),
            ("places", include_str!("../../ui/places.rs"), "Floater::new(\"places\","),
            ("minimap", include_str!("../../ui/minimap.rs"), "            \"minimap\","),
            ("worldmap", include_str!("../../ui/worldmap/mod.rs"), "Floater::new(\"worldmap\","),
            ("perf", include_str!("../../ui/perf.rs"), "            \"perf\","),
            ("gestures", include_str!("../../ui/gestures.rs"), "            \"gestures\","),
            ("appearance", include_str!("../../ui/appearance.rs"), "        \"appearance\","),
            ("ao", include_str!("../../ui/ao.rs"), "Floater::new(\"ao\","),
        ];
        for (id, source, made) in made_with {
            assert!(find(id).is_some(), "{id} is on the list");
            assert!(source.contains(made), "the window \"{id}\" is no longer made this way");
        }
        // the additional Inventory window: its fixed id through `floater_id`
        let inventory = include_str!("../../ui/inventory.rs");
        assert!(inventory.contains("const ADDITIONAL_WINDOW: Uuid = Uuid::from_u128(0x6b1f_2c4e_9a37_4d58_8e21_0f3c_5a90_1090);"));
        assert!(inventory.contains("format!(\"inventory_{id}\")"));
        let id = uuid::Uuid::from_u128(0x6b1f_2c4e_9a37_4d58_8e21_0f3c_5a90_1090);
        assert_eq!(ADDITIONAL_INVENTORY, format!("inventory_{id}"));
    }

    /// Each additional Inventory window is on the list once, with the id
    /// its slot is drawn with and its numbered title.
    #[test]
    fn every_additional_inventory_window_is_on_the_list() {
        use crate::ui::inventory::extra_windows;
        assert_eq!(INVENTORY_SLOTS.len(), usize::from(extra_windows::MAX_SLOTS));
        for (slot, id) in (1..=extra_windows::MAX_SLOTS).zip(INVENTORY_SLOTS) {
            assert_eq!(extra_windows::floater_of(slot).as_deref(), Some(id), "slot {slot}");
            let window = find(id).unwrap_or_else(|| panic!("slot {slot} is on the list"));
            assert_eq!(window.kind, Kind::Floater);
            assert_eq!(window.label(), extra_windows::slot_title(slot));
        }
        assert_eq!(
            find(ADDITIONAL_INVENTORY).map(|w| w.label()).as_deref(),
            Some(tr!("inventory-windows-title", number = 2).as_str())
        );
    }
}
