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
    /// The user's permissions refuse this class of tool.
    PermissionDenied,
    UnknownTool,
    InvalidArguments,
    /// Not logged in, or the inventory is not usable yet.
    NotReady,
    NotFound,
    /// A path names several folders.
    Ambiguous,
    /// The viewer did not answer in time.
    Timeout,
    /// The bridge is stopping.
    Unavailable,
    Internal,
}

impl ErrorCode {
    /// The HTTP status the bridge answers with.
    pub fn status(self) -> u16 {
        match self {
            ErrorCode::Unauthorized => 401,
            ErrorCode::Forbidden | ErrorCode::PermissionDenied => 403,
            ErrorCode::BadRequest | ErrorCode::InvalidArguments | ErrorCode::UnknownTool | ErrorCode::Ambiguous => 400,
            ErrorCode::NotFound => 404,
            ErrorCode::TooLarge => 413,
            ErrorCode::NotReady => 409,
            ErrorCode::Busy | ErrorCode::Unavailable => 503,
            ErrorCode::Timeout => 504,
            ErrorCode::Internal => 500,
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
}

/// Result caps the viewer applies (and the schemas advertise).
pub const SEARCH_DEFAULT: usize = 50;
pub const SEARCH_MAX: usize = 200;
pub const LIST_MAX: usize = 500;

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

/// The phase 1 tools: all read-only.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "viewer_status",
        description: "Whether the viewer is logged in, whether its inventory is usable, and how far the inventory fetch has gone. Works even when inventory reads are not allowed.",
        class: None,
        schema: no_arguments,
    },
    ToolSpec {
        name: "inventory_search",
        description: "Find inventory items and folders by name, optionally inside one folder or path, or of one type. Results are capped (see `truncated`). Names are not unique: use the returned ids.",
        class: Some(Class::Read),
        schema: search_schema,
    },
    ToolSpec {
        name: "inventory_list_folder",
        description: "List the subfolders and items of one inventory folder, by id or by path. Without arguments, lists My Inventory.",
        class: Some(Class::Read),
        schema: folder_schema,
    },
    ToolSpec {
        name: "inventory_item",
        description: "Details of one inventory item: name, description, type, folder, creator, creation date, permissions, whether it is worn. The asset id is given only for an item with full permissions, as in the viewer.",
        class: Some(Class::Read),
        schema: item_schema,
    },
    ToolSpec {
        name: "outfits_list",
        description: "The outfits saved in My Outfits, with the current outfit's base marked.",
        class: Some(Class::Read),
        schema: no_arguments,
    },
    ToolSpec {
        name: "worn_items",
        description: "What the avatar wears now: body parts, clothing and attached objects.",
        class: Some(Class::Read),
        schema: no_arguments,
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
        assert!(TOOLS.iter().filter(|t| t.name != "viewer_status").all(|t| t.class == Some(Class::Read)));
        assert!(tool("inventory_delete").is_none());
    }

    #[test]
    fn errors_serialize_with_snake_case_codes() {
        let e = ToolError::new(ErrorCode::PermissionDenied, "no");
        assert_eq!(serde_json::to_value(&e).expect("json"), json!({"code": "permission_denied", "message": "no"}));
        assert_eq!(e.to_string(), "permission_denied: no");
        assert_eq!(ErrorCode::Timeout.status(), 504);
    }
}
