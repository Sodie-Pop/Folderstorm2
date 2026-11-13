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
    label: fn() -> &'static str,
}

impl Window {
    /// Its name, as its title bar or its menu entry has it.
    pub fn label(&self) -> &'static str {
        (self.label)()
    }
}

/// The additional Inventory window (`ui/inventory.rs`, `ADDITIONAL_WINDOW`
/// through `floater_id`): one window with a fixed id.
pub const ADDITIONAL_INVENTORY: &str = "inventory_6b1f2c4e-9a37-4d58-8e21-0f3c5a901090";

const fn floater(id: &'static str, label: fn() -> &'static str) -> Window {
    Window {
        id,
        kind: Kind::Floater,
        label,
    }
}

static WINDOWS: &[Window] = &[
    floater("inventory", || tr!("common-inventory")),
    floater(ADDITIONAL_INVENTORY, || tr!("inventory-additional-window")),
    // geometry only: never its conversations or what they say
    floater("conversations", || tr!("common-conversations")),
    floater(crate::ui::people::WINDOW, || tr!("common-people")),
    floater("contacts", || tr!("common-contacts")),
    floater("places", || tr!("common-places")),
    floater(crate::ui::search::WINDOW, || tr!("search-title")),
    Window {
        id: "minimap",
        kind: Kind::Bare,
        label: || tr!("common-mini-map"),
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
}
