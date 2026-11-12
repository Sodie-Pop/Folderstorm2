//! Folderstorm: the inventory and outfit tools of phase 3, part 2
//! (folderstorm/plans/mcp.md, "Phase 3, part 2: inventory gaps"), after
//! Folderstorm (C++)'s `tools/fs-mcp` (`inventory_extra.go`,
//! `appearance.go`, `server.go`). Kept apart from the phase 1 and 2
//! catalogue so that the other phase 3 groups add theirs without touching
//! these; [`super::all`] lists both.

use super::{CHANGE_MAX, CHANGE_WAIT, ToolSpec};
use crate::policy::Class;
use serde_json::{Value, json};
use std::time::Duration;

/// Changes `inventory_changes` gives at most in one answer.
pub const CHANGES_MAX: usize = 200;
/// How long `inventory_landmark` waits for the landmark's asset and its
/// region.
pub const LANDMARK_WAIT: Duration = Duration::from_secs(10);
/// The item types `inventory_create_item` makes, as the inventory's New
/// menu does. Clothing and body parts are not among them: a new wearable
/// uploads its asset.
pub const CREATE_TYPES: &[&str] = &["notecard", "script", "gesture", "material", "sky", "water", "day_cycle", "landmark"];
/// `inventory_search`'s `filter_links` values.
pub const LINK_FILTERS: &[&str] = &["include", "exclude", "only"];

/// A tool of this group (one constructor, so that a field the catalogue
/// gains is given once for all of them).
const fn spec(name: &'static str, description: &'static str, class: Class, schema: fn() -> Value, wait: Duration) -> ToolSpec {
    ToolSpec {
        name,
        description,
        class: Some(class),
        also: None,
        schema,
        group: crate::binding::INVENTORY_GAPS,
        wait,
    }
}

fn ids_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": CHANGE_MAX,
                "description": "The items' and folders' UUIDs."}
        },
        "required": ["ids"],
        "additionalProperties": false
    })
}

fn system_folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {"type": "string", "description": "The system folder, e.g. \"textures\", \"landmarks\", \"objects\", \"trash\", \"current_outfit\" (see inventory_types)."}
        },
        "required": ["type"],
        "additionalProperties": false
    })
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn changes_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "since": {"type": "integer", "minimum": 0,
                "description": "The `generation` of an earlier answer: the changes after it. Without it: the current generation only, to start from."}
        },
        "additionalProperties": false
    })
}

fn landmark_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"item_id": {"type": "string", "description": "The landmark's inventory item UUID (a link to one is followed)."}},
        "required": ["item_id"],
        "additionalProperties": false
    })
}

fn outfit_items_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "outfit_id": {"type": "string", "description": "The outfit folder's UUID (see outfits_list)."},
            "path": {"type": "string", "description": "The outfit folder's path, e.g. \"My Outfits/Evening\"."}
        },
        "additionalProperties": false
    })
}

fn create_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {"type": "string", "enum": CREATE_TYPES,
                "description": "What to create. landmark: a landmark of where the avatar stands."},
            "name": {"type": "string", "description": "The new item's name (1 to 63 characters, no `|`). Default: the viewer's (\"New Script\"…, the parcel's name for a landmark)."},
            "folder_id": {"type": "string", "description": "The folder to create it in."},
            "path": {"type": "string", "description": "The folder's path. Neither: the folder of its type (Notecards, Scripts, Landmarks…)."}
        },
        "required": ["type"],
        "additionalProperties": false
    })
}

fn favorite_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": CHANGE_MAX,
                "description": "The items' and folders' UUIDs (a link marks its original)."},
            "favorite": {"type": "boolean", "description": "true: add to Favorites; false: remove from them."}
        },
        "required": ["ids", "favorite"],
        "additionalProperties": false
    })
}

fn batch_rename_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array", "minItems": 1, "maxItems": CHANGE_MAX,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "The item's or folder's UUID."},
                        "name": {"type": "string", "description": "Its new name (1 to 63 characters, no `|`)."}
                    },
                    "required": ["id", "name"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["items"],
        "additionalProperties": false
    })
}

fn thumbnail_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "The item's or folder's UUID whose picture changes."},
            "texture_id": {"type": "string", "description": "The texture to show: an inventory texture's item UUID that allows copying and transfer (or a Library texture)."},
            "clear": {"type": "boolean", "default": false, "description": "Remove the picture instead."}
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

/// The search filters phase 3 adds to `inventory_search`'s schema.
pub fn search_filters(properties: &mut Value) {
    let flag = |what: &str| json!({"type": "boolean", "description": what});
    let time = |what: &str| json!({"type": "integer", "description": what});
    let extra = json!({
        "description": {"type": "string", "description": "Text the description must contain (case-insensitive); items only."},
        "creator_id": {"type": "string", "description": "Only items made by this resident (UUID)."},
        "worn": flag("Only items worn (true) or not worn (false)."),
        "copyable": flag("Only items you may (true) or may not (false) copy."),
        "modifiable": flag("Only items you may (true) or may not (false) modify."),
        "favorite": flag("Only entries in (true) or not in (false) Favorites."),
        "created_after": time("Only items created at or after this time (Unix seconds)."),
        "created_before": time("Only items created before this time (Unix seconds)."),
        "filter_links": {"type": "string", "enum": LINK_FILTERS, "default": "include", "description": "Links too (include), no links (exclude) or links only (only)."},
        "linked_id": {"type": "string", "description": "Only the links that point at this item or folder (UUID)."}
    });
    if let (Some(properties), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            properties.insert(key.clone(), value.clone());
        }
    }
}

/// The names `inventory_search` takes on top of phase 1's.
pub const SEARCH_FILTERS: &[&str] = &[
    "description",
    "creator_id",
    "worn",
    "copyable",
    "modifiable",
    "favorite",
    "created_after",
    "created_before",
    "filter_links",
    "linked_id",
];

const NO_WAIT: Duration = Duration::ZERO;

pub const INVENTORY_TOOLS: &[ToolSpec] = &[
    spec(
        "inventory_get_many",
        "Details of up to 50 inventory items or folders by UUID in one call, as inventory_item and inventory_list_folder give them; ids not found are listed in `missing`.",
        Class::Read,
        ids_schema,
        NO_WAIT,
    ),
    spec(
        "inventory_system_folder",
        "The UUID of a system folder of My Inventory (Textures, Landmarks, Objects, Trash, Current Outfit…).",
        Class::Read,
        system_folder_schema,
        NO_WAIT,
    ),
    spec(
        "inventory_types",
        "The entry types, system folder names, wearable types and creatable item types the inventory tools accept.",
        Class::Read,
        no_arguments,
        NO_WAIT,
    ),
    spec(
        "inventory_protected_folders",
        "The folders an assistant cannot change, and why: system folders, folders locked in the inventory window, the viewer's pinned folders, the Marketplace Listings folder. Changes nothing.",
        Class::Read,
        no_arguments,
        NO_WAIT,
    ),
    spec(
        "inventory_changes",
        "What changed in My Inventory since a generation: items and folders added, changed or removed. Call it without `since` first to get a generation, then with `since` to poll. `reset` means the history was lost: read again what you need.",
        Class::Read,
        changes_schema,
        NO_WAIT,
    ),
    spec(
        "inventory_landmark",
        "Where a landmark points: its region, the position in the region and the global position.",
        Class::Read,
        landmark_schema,
        LANDMARK_WAIT,
    ),
    spec(
        "appearance_outfit_items",
        "The items of one outfit folder, links followed: name, type, wearable type, and whether each is worn now.",
        Class::Read,
        outfit_items_schema,
        NO_WAIT,
    ),
    spec(
        "inventory_create_item",
        "Create a new notecard, script, gesture, material, sky, water or day cycle, or a landmark of where the avatar stands, as the inventory's New menu does. Not in the Library, the Current Outfit, Received Items, the Trash, My Outfits or Lost And Found.",
        Class::Edit,
        create_item_schema,
        CHANGE_WAIT,
    ),
    spec(
        "inventory_set_favorite",
        "Add items or folders to Favorites, or remove them. Library and Trash entries cannot be marked.",
        Class::Edit,
        favorite_schema,
        CHANGE_WAIT,
    ),
    spec(
        "inventory_batch_rename",
        "Rename up to 50 items or folders in one change. All or nothing: if one cannot be renamed (system folders, links, calling cards, Library entries, items without Modify permission), none is.",
        Class::Edit,
        batch_rename_schema,
        CHANGE_WAIT,
    ),
    spec(
        "inventory_set_thumbnail",
        "Set the picture of an item or folder from a texture of the inventory that allows copying and transfer, as the item's Image window does (free), or remove it. Links, Library entries and items without Modify permission keep theirs.",
        Class::Edit,
        thumbnail_schema,
        CHANGE_WAIT,
    ),
];

#[cfg(test)]
mod tests {
    use super::super::{TOOLS, all, tool};
    use super::*;

    #[test]
    fn the_group_is_listed_once_with_its_classes() {
        let mut names = std::collections::HashSet::new();
        for t in all() {
            assert!(names.insert(t.name), "{} twice", t.name);
            assert_eq!((t.schema)()["type"], "object", "{}", t.name);
        }
        assert_eq!(names.len(), TOOLS.len() + INVENTORY_TOOLS.len() + super::super::world::WORLD_TOOLS.len());
        for read in [
            "inventory_get_many",
            "inventory_system_folder",
            "inventory_types",
            "inventory_protected_folders",
            "inventory_changes",
            "inventory_landmark",
            "appearance_outfit_items",
        ] {
            let t = tool(read).expect(read);
            assert_eq!(t.class, Some(Class::Read), "{read}");
            assert!(!t.changes(), "{read} reads only");
        }
        for edit in [
            "inventory_create_item",
            "inventory_set_favorite",
            "inventory_batch_rename",
            "inventory_set_thumbnail",
        ] {
            let t = tool(edit).expect(edit);
            assert_eq!(t.class, Some(Class::Edit), "{edit}");
            assert!(t.changes(), "{edit} may ask");
            assert_eq!(t.wait, CHANGE_WAIT);
        }
        // never a way to the next owner's rights, to uploads or to L$
        for absent in ["inventory_next_owner_set", "inventory_snapshot_upload", "inventory_upload_execute"] {
            assert!(tool(absent).is_none(), "{absent}");
        }
    }

    #[test]
    fn the_search_takes_the_new_filters() {
        let mut properties = json!({"query": {"type": "string"}});
        search_filters(&mut properties);
        for key in SEARCH_FILTERS {
            assert!(properties.get(*key).is_some(), "{key}");
        }
        assert!(properties.get("query").is_some(), "phase 1's are kept");
        assert_eq!(properties["filter_links"]["enum"], json!(LINK_FILTERS));
        assert_eq!(create_item_schema()["properties"]["type"]["enum"], json!(CREATE_TYPES));
    }
}
