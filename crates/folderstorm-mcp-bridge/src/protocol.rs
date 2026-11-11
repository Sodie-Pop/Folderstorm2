//! The wire between the sidecar and the bridge, and the tool catalogue both
//! sides share: the sidecar lists it to the MCP client, the bridge checks
//! each call's permission class with it.
//!
//! - `GET /v1/health` → `{"ok":true,"result":Health}`: the token is valid
//!   and the bridge listens (needs no permission class);
//! - `POST /v1/call` with [`CallRequest`] → `{"ok":true,"result":…}` or
//!   `{"ok":false,"error":ToolError}`.

use crate::policy::Class;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallRequest {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// `/v1/health`'s result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Health {
    pub api: u32,
    pub product: String,
    pub version: String,
    /// Whether inventory reads are allowed right now.
    pub read_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// No token, or the wrong one.
    Unauthorized,
    /// A browser's request (Origin header) or a foreign Host header.
    Forbidden,
    BadRequest,
    TooLarge,
    /// Too many connections or calls waiting.
    Busy,
    /// The user's permissions refuse this class of tool, the user refused
    /// it in the Ask dialog or let it time out, or it is never allowed
    /// (permanent delete, Empty Trash).
    PermissionDenied,
    /// The viewer's own rules refuse the change, as its windows would: a
    /// protected folder, an item without Copy or Modify, an RLV
    /// restriction…
    NotAllowed,
    UnknownTool,
    InvalidArguments,
    /// Not logged in, or the inventory is not usable yet.
    NotReady,
    NotFound,
    /// A path names several folders.
    Ambiguous,
    /// The viewer did not answer in time, or a change was sent and the
    /// server did not confirm it in time (it may still land).
    Timeout,
    /// The change was sent and the server refused it.
    Failed,
    /// The bridge is stopping.
    Unavailable,
    Internal,
}

impl ErrorCode {
    /// The HTTP status the bridge answers with.
    pub fn status(self) -> u16 {
        match self {
            ErrorCode::Unauthorized => 401,
            ErrorCode::Forbidden | ErrorCode::PermissionDenied | ErrorCode::NotAllowed => 403,
            ErrorCode::BadRequest | ErrorCode::InvalidArguments | ErrorCode::UnknownTool | ErrorCode::Ambiguous => 400,
            ErrorCode::NotFound => 404,
            ErrorCode::TooLarge => 413,
            ErrorCode::NotReady => 409,
            ErrorCode::Busy | ErrorCode::Unavailable => 503,
            ErrorCode::Timeout => 504,
            ErrorCode::Internal => 500,
            ErrorCode::Failed => 502,
        }
    }
}

/// An error a tool call ends with: a fixed code and a short English message
/// for the assistant. Messages never carry a token or a request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ToolError {
        ToolError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = serde_json::to_value(self.code).ok();
        let code = code.as_ref().and_then(Value::as_str).unwrap_or("error");
        write!(f, "{code}: {}", self.message)
    }
}

impl std::error::Error for ToolError {}

/// One tool of the catalogue.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// The permission it needs; None for the health check.
    pub class: Option<Class>,
    /// Its JSON Schema (MCP `inputSchema`).
    pub schema: fn() -> Value,
    /// How long the viewer may take on top of the usual wait once the call
    /// runs: a change waits for the server, `wait_for_bake` for the avatar.
    pub wait: Duration,
}

impl ToolSpec {
    /// Whether the tool changes something (it may then ask the user).
    pub fn changes(&self) -> bool {
        self.class.is_some_and(Class::can_ask)
    }
}

/// Result caps the viewer applies (and the schemas advertise).
pub const SEARCH_DEFAULT: usize = 50;
pub const SEARCH_MAX: usize = 200;
pub const LIST_MAX: usize = 500;
/// Entries one change may name.
pub const CHANGE_MAX: usize = 50;
/// How long the Ask dialog waits for the user before refusing.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a change waits for the server's confirmation.
pub const CHANGE_WAIT: Duration = Duration::from_secs(30);
/// `wait_for_bake`'s default and longest wait, in seconds.
pub const BAKE_WAIT_DEFAULT: u64 = 30;
pub const BAKE_WAIT_MAX: u64 = 120;
const NO_WAIT: Duration = Duration::ZERO;

/// Tools that are never run, whatever the permissions: permanent delete,
/// Purge and Empty Trash. Asking for one answers `permission_denied`.
pub const NEVER: &[&str] = &["inventory_delete", "inventory_purge", "empty_trash"];
pub const NEVER_MESSAGE: &str = "permanent delete and Empty Trash are never allowed to an assistant: use inventory_trash";

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Text the name must contain (case-insensitive). Empty matches every name, within the filters."},
            "folder_id": {"type": "string", "description": "Only search inside this folder (UUID) and its subfolders."},
            "path": {"type": "string", "description": "Only search inside this folder path, e.g. \"Clothes/Boots\" or \"Library/Clothing\"."},
            "type": {"type": "string", "description": "Only this kind of entry.", "enum": crate::protocol::KINDS},
            "include_library": {"type": "boolean", "default": false},
            "include_trash": {"type": "boolean", "default": false},
            "limit": {"type": "integer", "minimum": 1, "maximum": SEARCH_MAX, "default": SEARCH_DEFAULT}
        },
        "additionalProperties": false
    })
}

fn folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "folder_id": {"type": "string", "description": "The folder's UUID."},
            "path": {"type": "string", "description": "The folder's path from My Inventory, e.g. \"Clothes/Boots\", or from \"Library/…\". Neither: My Inventory itself."}
        },
        "additionalProperties": false
    })
}

fn item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"item_id": {"type": "string", "description": "The inventory item's UUID."}},
        "required": ["item_id"],
        "additionalProperties": false
    })
}

const FOLDER_ID: &str = "The folder's UUID.";
const FOLDER_PATH: &str = "The folder's path from My Inventory, e.g. \"Clothes/Boots\".";

fn ids_schema(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": CHANGE_MAX, "description": what})
}

fn create_folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "The new folder's name (1 to 63 characters, no `|`)."},
            "folder_id": {"type": "string", "description": "The parent folder's UUID."},
            "path": {"type": "string", "description": "The parent folder's path. Neither: My Inventory."}
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn rename_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "The item's or folder's UUID."},
            "name": {"type": "string", "description": "The new name (1 to 63 characters, no `|`)."}
        },
        "required": ["id", "name"],
        "additionalProperties": false
    })
}

fn describe_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "item_id": {"type": "string", "description": "The item's UUID."},
            "description": {"type": "string", "maxLength": 127, "description": "The new description; empty clears it."}
        },
        "required": ["item_id", "description"],
        "additionalProperties": false
    })
}

fn to_folder_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ids": ids_schema("The items' and folders' UUIDs."),
            "folder_id": {"type": "string", "description": FOLDER_ID},
            "path": {"type": "string", "description": FOLDER_PATH}
        },
        "required": ["ids"],
        "additionalProperties": false
    })
}

fn ids_only_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"ids": ids_schema("The items' and folders' UUIDs.")},
        "required": ["ids"],
        "additionalProperties": false
    })
}

fn outfit_schema(modes: Option<(&[&str], &str)>) -> Value {
    let mut properties = json!({
        "outfit_id": {"type": "string", "description": "The outfit folder's UUID (see outfits_list)."},
        "path": {"type": "string", "description": "The outfit folder's path, e.g. \"My Outfits/Evening\"."}
    });
    if let Some((modes, default)) = modes {
        properties["mode"] = json!({"type": "string", "enum": modes, "default": default,
            "description": "replace: wear this outfit instead of the current one; add: wear it over what is worn."});
    }
    json!({"type": "object", "properties": properties, "additionalProperties": false})
}

fn outfit_wear_schema() -> Value {
    outfit_schema(Some((&["replace", "add"], "replace")))
}

fn outfit_take_off_schema() -> Value {
    outfit_schema(None)
}

fn items_schema(modes: bool) -> Value {
    let mut properties = json!({"item_ids": ids_schema("The inventory items' UUIDs (links are followed).")});
    if modes {
        properties["mode"] = json!({"type": "string", "enum": ["add", "replace"], "default": "add",
            "description": "add: wear over what is worn; replace: take off what is worn on the same layer or attachment point."});
    }
    json!({"type": "object", "properties": properties, "required": ["item_ids"], "additionalProperties": false})
}

fn items_wear_schema() -> Value {
    items_schema(true)
}

fn items_take_off_schema() -> Value {
    items_schema(false)
}

fn bake_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"timeout_seconds": {"type": "integer", "minimum": 1, "maximum": BAKE_WAIT_MAX, "default": BAKE_WAIT_DEFAULT}},
        "additionalProperties": false
    })
}

/// The `type` values of search results and filters.
pub const KINDS: &[&str] = &[
    "folder",
    "texture",
    "sound",
    "calling_card",
    "landmark",
    "clothing",
    "object",
    "notecard",
    "script",
    "body_part",
    "animation",
    "gesture",
    "snapshot",
    "link",
    "folder_link",
    "settings",
    "material",
    "other",
];

/// The tools: the reads of phase 1, then the changes of phase 2.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "viewer_status",
        description: "Whether the viewer is logged in, whether its inventory is usable, and how far the inventory fetch has gone. Works even when inventory reads are not allowed.",
        class: None,
        schema: no_arguments,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_search",
        description: "Find inventory items and folders by name, optionally inside one folder or path, or of one type. Results are capped (see `truncated`). Names are not unique: use the returned ids.",
        class: Some(Class::Read),
        schema: search_schema,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_list_folder",
        description: "List the subfolders and items of one inventory folder, by id or by path. Without arguments, lists My Inventory.",
        class: Some(Class::Read),
        schema: folder_schema,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_item",
        description: "Details of one inventory item: name, description, type, folder, creator, creation date, permissions, whether it is worn. The asset id is given only for an item with full permissions, as in the viewer.",
        class: Some(Class::Read),
        schema: item_schema,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "outfits_list",
        description: "The outfits saved in My Outfits, with the current outfit's base marked.",
        class: Some(Class::Read),
        schema: no_arguments,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "worn_items",
        description: "What the avatar wears now: body parts, clothing and attached objects.",
        class: Some(Class::Read),
        schema: no_arguments,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_create_folder",
        description: "Create a folder, in My Inventory or inside another folder. Not in the Library, the Current Outfit, Received Items, the Trash or My Outfits.",
        class: Some(Class::Edit),
        schema: create_folder_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_rename",
        description: "Rename an item or a folder. System folders, links, calling cards, Library entries and items without Modify permission cannot be renamed.",
        class: Some(Class::Edit),
        schema: rename_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_describe",
        description: "Change an item's description. Items without Modify permission, links and Library items cannot be changed.",
        class: Some(Class::Edit),
        schema: describe_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_move",
        description: "Move items and folders into another folder. System, locked and worn entries stay. To delete, use inventory_trash.",
        class: Some(Class::Move),
        schema: to_folder_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_copy",
        description: "Copy items and folders into another folder. Items without Copy permission cannot be copied; folders must be fully loaded.",
        class: Some(Class::Move),
        schema: to_folder_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_link",
        description: "Create links to items or folders in another folder (an outfit folder only links what can be worn).",
        class: Some(Class::Move),
        schema: to_folder_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_trash",
        description: "Move items and folders to the Trash, as Delete does in the inventory. Nothing is deleted for good: inventory_restore brings them back. Worn items and system folders stay.",
        class: Some(Class::Trash),
        schema: ids_only_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_restore",
        description: "Move items and folders out of the Trash, to the folder of their type (\"Restore item\").",
        class: Some(Class::Trash),
        schema: ids_only_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "outfit_wear",
        description: "Wear a saved outfit, instead of the current one (replace) or over it (add). Then call wait_for_bake.",
        class: Some(Class::Wear),
        schema: outfit_wear_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "outfit_take_off",
        description: "Take off what an outfit folder holds. Body parts always stay.",
        class: Some(Class::Wear),
        schema: outfit_take_off_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "items_wear",
        description: "Wear clothing, body parts or objects from the inventory. Library items must be copied first.",
        class: Some(Class::Wear),
        schema: items_wear_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "items_take_off",
        description: "Take off worn clothing or attached objects. Body parts cannot be taken off, only replaced.",
        class: Some(Class::Wear),
        schema: items_take_off_schema,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "wait_for_bake",
        description: "Wait until the last outfit change is over and the avatar's appearance is up to date, or the timeout. Changes nothing.",
        class: Some(Class::Read),
        schema: bake_schema,
        wait: Duration::from_secs(BAKE_WAIT_MAX),
    },
];

pub fn tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_object_schema_and_a_unique_name() {
        let mut names = std::collections::HashSet::new();
        for t in TOOLS {
            assert!(names.insert(t.name), "{} twice", t.name);
            let schema = (t.schema)();
            assert_eq!(schema["type"], "object", "{}", t.name);
            assert!(!t.description.is_empty());
        }
        assert!(tool("viewer_status").is_some_and(|t| t.class.is_none()));
        assert!(TOOLS.iter().filter(|t| t.name != "viewer_status").all(|t| t.class.is_some()));
        // what waits for the server or the avatar says so
        assert!(TOOLS.iter().filter(|t| t.changes()).all(|t| t.wait == CHANGE_WAIT));
        assert!(TOOLS.iter().filter(|t| t.class == Some(Class::Read)).all(|t| !t.changes()));
        for never in NEVER {
            assert!(tool(never).is_none(), "{never} is never a tool");
        }
    }

    #[test]
    fn errors_serialize_with_snake_case_codes() {
        let e = ToolError::new(ErrorCode::PermissionDenied, "no");
        assert_eq!(
            serde_json::to_value(&e).expect("json"),
            json!({"code": "permission_denied", "message": "no"})
        );
        assert_eq!(e.to_string(), "permission_denied: no");
        assert_eq!(ErrorCode::Timeout.status(), 504);
    }
}
