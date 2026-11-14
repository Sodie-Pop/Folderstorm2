//! The wire between the sidecar and the bridge, and the tool catalogue both
//! sides share: the sidecar lists it to the MCP client, the bridge checks
//! each call's permission class with it.
//!
//! - `GET /v1/health` → `{"ok":true,"result":Health}`: the token is valid
//!   and the bridge listens (needs no permission class);
//! - `POST /v1/call` with [`CallRequest`] → `{"ok":true,"result":…}` or
//!   `{"ok":false,"error":ToolError}`.
//!
//! Every field added after the first release is optional and skipped when
//! empty, so that an older side reads the wire as before (serde ignores the
//! fields it does not know); [`crate::binding`] says how each side learns
//! what the other one knows.

use crate::binding::{self, Capabilities};
use crate::policy::Class;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

// Folderstorm: phase 3, part 2 (inventory gaps), its own catalogue
pub mod inventory;

// Folderstorm: phase 3, part 3 (world observation), its own catalogue
pub mod world;

// Folderstorm: receipts, plans and operations (mcp-receipts.md)
pub mod assistant;

// Folderstorm: object inspection and lifecycle (mcp-objects.md)
pub mod objects;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallRequest {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
    /// The `viewer_session` the sidecar pinned: a bound tool runs only on
    /// that login ([`ToolSpec::bound`]). None: unbound, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The sidecar's own capabilities. None: a sidecar older than
    /// capability versions, which gets only the error codes it knows
    /// ([`ErrorCode::for_sidecar`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Capabilities>,
}

/// `/v1/health`'s result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Health {
    pub api: u32,
    pub product: String,
    pub version: String,
    /// Whether inventory reads are allowed right now.
    pub read_allowed: bool,
    /// The tool groups this viewer implements, and their versions. None: a
    /// viewer older than capability versions ([`binding::assumed`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    /// The login the viewer runs bound calls on now; None while logged out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_session: Option<String>,
}

impl Health {
    /// What the viewer implements, a viewer without the field counting as
    /// [`binding::assumed`].
    pub fn capabilities(&self) -> Capabilities {
        self.capabilities.clone().unwrap_or_else(binding::assumed)
    }
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
    /// The call was bound to another login than the viewer's now (logged
    /// out, in again, another character, or the bridge restarted): nothing
    /// was sent. The error carries the current `viewer_session`.
    SessionChanged,
    /// The running viewer does not implement this tool (it is older than
    /// the sidecar): nothing was sent.
    Unsupported,
    /// Folderstorm: a `client_request_id` already used with other
    /// arguments, or retired: nothing was run (receipts).
    Conflict,
    /// Folderstorm: the world changed since the plan or the dialog (a
    /// plan's rows no longer match): nothing was sent (plans).
    Stale,
    /// Folderstorm: stopped by the assistant (`assistant_operation_cancel`)
    /// or the user (Stop in Preferences), or its time ran out, before it
    /// was sent (operations).
    Cancelled,
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
            ErrorCode::NotReady | ErrorCode::SessionChanged | ErrorCode::Conflict | ErrorCode::Stale | ErrorCode::Cancelled => 409,
            ErrorCode::Unsupported => 501,
            ErrorCode::Busy | ErrorCode::Unavailable => 503,
            ErrorCode::Timeout => 504,
            ErrorCode::Internal => 500,
            ErrorCode::Failed => 502,
        }
    }

    /// A code added after the first release: the capability a sidecar
    /// declares (`CallRequest::features`) to be sent it, and the older code
    /// an older sidecar gets instead, since it reads an unknown code as an
    /// unreadable error. None: every sidecar knows it.
    pub fn fallback(self) -> Option<(&'static str, ErrorCode)> {
        match self {
            // both came with login binding, as `features` itself did
            ErrorCode::SessionChanged => Some((binding::LOGIN_BINDING, ErrorCode::NotReady)),
            ErrorCode::Unsupported => Some((binding::LOGIN_BINDING, ErrorCode::UnknownTool)),
            // Folderstorm: receipts (mcp-receipts.md, section 3)
            ErrorCode::Conflict => Some((binding::RECEIPTS, ErrorCode::InvalidArguments)),
            ErrorCode::Stale => Some((binding::PLANS, ErrorCode::NotAllowed)),
            ErrorCode::Cancelled => Some((binding::OPERATIONS, ErrorCode::PermissionDenied)),
            _ => None,
        }
    }

    /// This code as a sidecar declaring `features` may be sent it.
    pub fn for_sidecar(self, features: Option<&Capabilities>) -> ErrorCode {
        match self.fallback() {
            Some((needs, older)) if !features.is_some_and(|f| f.contains_key(needs)) => older,
            _ => self,
        }
    }
}

/// An error a tool call ends with: a fixed code and a short English message
/// for the assistant. Messages never carry a token or a request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    /// `session_changed`: the viewer's `viewer_session` now (None while
    /// logged out), which the sidecar pins next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_session: Option<String>,
    /// Folderstorm: the operation of a change the viewer took (its
    /// receipt, `assistant_operation_status`), also in the listener's own
    /// timeout. Kept for every sidecar: an older one ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ToolError {
        ToolError {
            code,
            message: message.into(),
            viewer_session: None,
            operation_id: None,
        }
    }

    /// This error as a sidecar declaring `features` may be sent it: an
    /// older sidecar gets an older code, and today's fields only.
    pub fn for_sidecar(mut self, features: Option<&Capabilities>) -> ToolError {
        let code = self.code.for_sidecar(features);
        if code != self.code {
            self.code = code;
            self.viewer_session = None;
        }
        self
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = serde_json::to_value(self.code).ok();
        let code = code.as_ref().and_then(Value::as_str).unwrap_or("error");
        write!(f, "{code}: {}", self.message)?;
        match &self.operation_id {
            Some(operation) => write!(f, " (operation_id {operation})"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ToolError {}

/// One tool of the catalogue.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// The permission it needs; None for the health check.
    pub class: Option<Class>,
    /// A second permission it needs too (a thumbnail from a picture: Edit
    /// and Camera); see `Permissions::decide_all`.
    pub also: Option<Class>,
    /// The capability ([`crate::binding`]) whose version 1 brings it: the
    /// sidecar sends it only to a viewer that advertises that group.
    pub group: &'static str,
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

    /// Every permission class it needs.
    pub fn classes(&self) -> impl Iterator<Item = Class> {
        self.class.into_iter().chain(self.also)
    }

    /// Whether a call of it runs only on the login the sidecar pinned
    /// (`CallRequest::session`). The health check (no class) answers on
    /// whatever login is current: it is how the assistant looks again after
    /// `session_changed`.
    pub fn bound(&self) -> bool {
        // Folderstorm: the receipts' and operations' tools need no class
        // but belong to one login (mcp-receipts.md, section 2)
        self.class.is_some() || assistant::bound_group(self.group)
    }

    /// Folderstorm: its JSON Schema as the sidecar lists it: a change also
    /// takes an optional `client_request_id` (receipts), which the viewer
    /// takes out before the tool reads its arguments.
    pub fn input_schema(&self) -> Value {
        let mut schema = (self.schema)();
        if self.changes()
            && let Some(properties) = schema["properties"].as_object_mut()
        {
            properties.insert(crate::receipts::CLIENT_REQUEST_ID.to_owned(), assistant::client_request_id_schema());
        }
        // Folderstorm: the object tools' changes need their id (mcp-objects.md)
        if objects::needs_client_request_id(self.name) {
            match schema["required"].as_array_mut() {
                Some(required) => required.push(json!(crate::receipts::CLIENT_REQUEST_ID)),
                None => schema["required"] = json!([crate::receipts::CLIENT_REQUEST_ID]),
            }
        }
        schema
    }
}

/// Calls that ask the user even when their classes are on Allow: a picture
/// with the viewer's interface shows its windows, chat included. Never
/// still refuses them.
pub fn always_asks(tool: &str, arguments: &Value) -> bool {
    tool == "camera_snapshot" && arguments["interface"].as_bool() == Some(true)
        // Folderstorm: removing from an object, taking an object (mcp-objects.md)
        || objects::always_asks(tool)
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
/// How long a camera move may take to settle before it answers.
pub const CAMERA_WAIT: Duration = Duration::from_secs(5);
/// How long a picture may take: the camera, a photo already being drawn,
/// its passes and its encoding.
pub const SNAPSHOT_WAIT: Duration = Duration::from_secs(45);
/// The camera presets of `camera_preset`.
pub const CAMERA_PRESETS: &[&str] = &["portrait", "full_body", "front", "back", "left", "right"];
/// A picture's sides, in pixels.
pub const SNAPSHOT_EDGE_MIN: u32 = 64;
pub const SNAPSHOT_EDGE_MAX: u32 = 2048;
pub const SNAPSHOT_EDGE_DEFAULT: u32 = 1024;
/// The largest encoded picture sent (4 MB once in base64, well under the
/// client's 8 MiB answer).
pub const IMAGE_BYTES_MAX: usize = 3_000_000;
/// The field of view an exact shot may ask for, in degrees.
pub const FOV_DEGREES: (f32, f32) = (5.0, 120.0);
/// The key of a result that carries a picture: `{"mime_type", "data"}`
/// (base64), which the sidecar sends as MCP image content.
pub const IMAGE_KEY: &str = "image";

/// Tools that are never run, whatever the permissions: permanent delete,
/// Purge and Empty Trash. Asking for one answers `permission_denied`.
pub const NEVER: &[&str] = &["inventory_delete", "inventory_purge", "empty_trash"];
pub const NEVER_MESSAGE: &str = "permanent delete and Empty Trash are never allowed to an assistant: use inventory_trash";

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn search_schema() -> Value {
    let mut schema = json!({
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
    });
    inventory::search_filters(&mut schema["properties"]); // Folderstorm: phase 3 filters
    schema
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

fn camera_preset_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "preset": {"type": "string", "enum": CAMERA_PRESETS,
                "description": "portrait: head and shoulders from the front; full_body: the whole avatar from the front, a little above; front, back, left, right: the whole avatar from that side, level."}
        },
        "required": ["preset"],
        "additionalProperties": false
    })
}

fn vector_schema(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": what})
}

fn camera_shot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "position": vector_schema("Where the camera stands, in region coordinates (metres). Give `target` with it."),
            "target": vector_schema("The point the camera looks at, in region coordinates (metres)."),
            "camera_offset": vector_schema("Instead of `position`: the camera relative to the avatar, in metres in its frame (x forward, y left, z up)."),
            "focus_offset": vector_schema("With `camera_offset`: the point looked at, relative to the avatar in its frame. Default: the avatar's centre."),
            "fov_degrees": {"type": "number", "minimum": FOV_DEGREES.0, "maximum": FOV_DEGREES.1,
                "description": "Vertical field of view. The user's own comes back with camera_reset or camera_preset."}
        },
        "additionalProperties": false
    })
}

fn snapshot_schema() -> Value {
    let edge = |what: &str| json!({"type": "integer", "minimum": SNAPSHOT_EDGE_MIN, "maximum": SNAPSHOT_EDGE_MAX, "description": what});
    let fraction = json!({"type": "number", "minimum": 0.0, "maximum": 1.0});
    json!({
        "type": "object",
        "properties": {
            "source": {"type": "string", "enum": ["window", "world"], "default": "window",
                "description": "window: the window's proportions; world: the world view only (the custom world viewport when one is set)."},
            "width": edge("The picture's width. With `height`: that size, centre-cropped to its proportions. Alone: the height follows."),
            "height": edge("The picture's height."),
            "max_edge": {"type": "integer", "minimum": SNAPSHOT_EDGE_MIN, "maximum": SNAPSHOT_EDGE_MAX, "default": SNAPSHOT_EDGE_DEFAULT,
                "description": "Without width and height: the long edge at most."},
            "crop": {"type": "object", "description": "The part of the source kept, in fractions of its width and height (0 to 1).",
                "properties": {"x": fraction, "y": fraction, "width": fraction, "height": fraction},
                "required": ["x", "y", "width", "height"], "additionalProperties": false},
            "interface": {"type": "boolean", "default": false,
                "description": "Include the viewer's windows (chat included) as on screen; window source only. Always asks the user first."},
            "huds": {"type": "boolean", "default": false, "description": "Include the HUDs."},
            "format": {"type": "string", "enum": ["jpeg", "png"], "default": "jpeg"},
            "save": {"type": "boolean", "default": false, "description": "Also save the picture in the viewer's photo folder."}
        },
        "additionalProperties": false
    })
}

fn thumbnail_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "The outfit folder's, folder's or item's UUID."},
            "path": {"type": "string", "description": "Instead of `id`: a folder's path, e.g. \"My Outfits/Evening\"."}
        },
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

/// The tools: the reads of phase 1, the changes of phase 2, then the
/// camera and pictures of phase 3.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "viewer_status",
        description: "Whether the viewer is logged in, whether its inventory is usable, and how far the inventory fetch has gone. Works even when inventory reads are not allowed.",
        class: None,
        also: None,
        schema: no_arguments,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_search",
        description: "Find inventory items and folders by name, optionally inside one folder or path, or of one type. Results are capped (see `truncated`). Names are not unique: use the returned ids.",
        class: Some(Class::Read),
        also: None,
        schema: search_schema,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_list_folder",
        description: "List the subfolders and items of one inventory folder, by id or by path. Without arguments, lists My Inventory.",
        class: Some(Class::Read),
        also: None,
        schema: folder_schema,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_item",
        description: "Details of one inventory item: name, description, type, folder, creator, creation date, permissions, whether it is worn. The asset id is given only for an item with full permissions, as in the viewer.",
        class: Some(Class::Read),
        also: None,
        schema: item_schema,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "outfits_list",
        description: "The outfits saved in My Outfits, with the current outfit's base marked.",
        class: Some(Class::Read),
        also: None,
        schema: no_arguments,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "worn_items",
        description: "What the avatar wears now: body parts, clothing and attached objects.",
        class: Some(Class::Read),
        also: None,
        schema: no_arguments,
        group: binding::INVENTORY,
        wait: NO_WAIT,
    },
    ToolSpec {
        name: "inventory_create_folder",
        description: "Create a folder, in My Inventory or inside another folder. Not in the Library, the Current Outfit, Received Items, the Trash or My Outfits.",
        class: Some(Class::Edit),
        also: None,
        schema: create_folder_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_rename",
        description: "Rename an item or a folder. System folders, links, calling cards, Library entries and items without Modify permission cannot be renamed.",
        class: Some(Class::Edit),
        also: None,
        schema: rename_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_describe",
        description: "Change an item's description. Items without Modify permission, links and Library items cannot be changed.",
        class: Some(Class::Edit),
        also: None,
        schema: describe_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_move",
        description: "Move items and folders into another folder. System, locked and worn entries stay. To delete, use inventory_trash.",
        class: Some(Class::Move),
        also: None,
        schema: to_folder_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_copy",
        description: "Copy items and folders into another folder. Items without Copy permission cannot be copied; folders must be fully loaded.",
        class: Some(Class::Move),
        also: None,
        schema: to_folder_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_link",
        description: "Create links to items or folders in another folder (an outfit folder only links what can be worn).",
        class: Some(Class::Move),
        also: None,
        schema: to_folder_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_trash",
        description: "Move items and folders to the Trash, as Delete does in the inventory. Nothing is deleted for good: inventory_restore brings them back. Worn items and system folders stay.",
        class: Some(Class::Trash),
        also: None,
        schema: ids_only_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "inventory_restore",
        description: "Move items and folders out of the Trash, to the folder of their type (\"Restore item\").",
        class: Some(Class::Trash),
        also: None,
        schema: ids_only_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "outfit_wear",
        description: "Wear a saved outfit, instead of the current one (replace) or over it (add). Then call wait_for_bake.",
        class: Some(Class::Wear),
        also: None,
        schema: outfit_wear_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "outfit_take_off",
        description: "Take off what an outfit folder holds. Body parts always stay.",
        class: Some(Class::Wear),
        also: None,
        schema: outfit_take_off_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "items_wear",
        description: "Wear clothing, body parts or objects from the inventory. Library items must be copied first.",
        class: Some(Class::Wear),
        also: None,
        schema: items_wear_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "items_take_off",
        description: "Take off worn clothing or attached objects. Body parts cannot be taken off, only replaced.",
        class: Some(Class::Wear),
        also: None,
        schema: items_take_off_schema,
        group: binding::INVENTORY,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: "wait_for_bake",
        description: "Wait until the last outfit change is over and the avatar's appearance is up to date, or the timeout. Changes nothing.",
        class: Some(Class::Read),
        also: None,
        schema: bake_schema,
        group: binding::INVENTORY,
        wait: Duration::from_secs(BAKE_WAIT_MAX),
    },
    ToolSpec {
        name: "camera_preset",
        description: "Frame your own avatar: portrait, full_body, front, back, left or right. Answers once the camera has stopped. Refused under an RLV camera restriction.",
        class: Some(Class::Camera),
        also: None,
        schema: camera_preset_schema,
        group: binding::CAMERA,
        wait: CAMERA_WAIT,
    },
    ToolSpec {
        name: "camera_shot",
        description: "Place the camera exactly: position and target in region coordinates, or offsets from your avatar in its frame, with an optional field of view. Within the draw distance of the avatar.",
        class: Some(Class::Camera),
        also: None,
        schema: camera_shot_schema,
        group: binding::CAMERA,
        wait: CAMERA_WAIT,
    },
    ToolSpec {
        name: "camera_reset",
        description: "Put the camera back behind the avatar, as Escape does, with the user's own field of view.",
        class: Some(Class::Camera),
        also: None,
        schema: no_arguments,
        group: binding::CAMERA,
        wait: CAMERA_WAIT,
    },
    ToolSpec {
        name: "camera_snapshot",
        description: "Take a picture of the world as the camera sees it, at a chosen size and crop, and get it as an image (JPEG or PNG, 3 MB at most). No interface or HUDs unless asked; asking for the interface always asks the user. Optionally saved in the viewer's photo folder.",
        class: Some(Class::Camera),
        also: None,
        schema: snapshot_schema,
        group: binding::CAMERA,
        wait: SNAPSHOT_WAIT,
    },
    ToolSpec {
        name: "inventory_snapshot_thumbnail",
        description: "Set the picture of an outfit, a folder or an item from what the camera sees now (square, 256 x 256), as the Outfit Gallery's Take snapshot does. Free. Frame it first with camera_preset or camera_shot.",
        class: Some(Class::Edit),
        also: Some(Class::Camera),
        schema: thumbnail_schema,
        group: binding::CAMERA,
        wait: Duration::from_secs(SNAPSHOT_WAIT.as_secs() + CHANGE_WAIT.as_secs()),
    },
];

pub fn tool(name: &str) -> Option<&'static ToolSpec> {
    all().find(|t| t.name == name)
}

/// Every tool: [`TOOLS`], then the groups phase 3 adds.
pub fn all() -> impl Iterator<Item = &'static ToolSpec> {
    TOOLS
        .iter()
        .chain(inventory::INVENTORY_TOOLS)
        .chain(world::WORLD_TOOLS) // Folderstorm: phase 3 groups
        .chain(assistant::ASSISTANT_TOOLS) // Folderstorm: receipts, plans, operations
        .chain(objects::OBJECT_TOOLS) // Folderstorm: object inspection and lifecycle
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
        // what waits for the server, the camera or the photo says so
        assert!(TOOLS.iter().filter(|t| t.changes()).all(|t| t.wait >= CAMERA_WAIT));
        assert!(
            TOOLS
                .iter()
                .filter(|t| t.class == Some(Class::Camera))
                .all(|t| t.wait >= CAMERA_WAIT)
        );
        assert!(TOOLS.iter().filter(|t| t.class == Some(Class::Read)).all(|t| !t.changes()));
        for never in NEVER {
            assert!(tool(never).is_none(), "{never} is never a tool");
        }
    }

    #[test]
    fn the_camera_tools_and_the_thumbnail_need_their_classes() {
        for name in ["camera_preset", "camera_shot", "camera_reset", "camera_snapshot"] {
            let t = tool(name).expect(name);
            assert_eq!(t.classes().collect::<Vec<_>>(), vec![Class::Camera], "{name}");
            assert!(t.changes(), "{name} may ask");
        }
        let thumbnail = tool("inventory_snapshot_thumbnail").expect("thumbnail");
        assert_eq!(thumbnail.classes().collect::<Vec<_>>(), vec![Class::Edit, Class::Camera]);
        assert!(thumbnail.wait > SNAPSHOT_WAIT);
        let presets = camera_preset_schema();
        assert_eq!(presets["properties"]["preset"]["enum"], json!(CAMERA_PRESETS));
        assert_eq!(snapshot_schema()["properties"]["width"]["maximum"], SNAPSHOT_EDGE_MAX);
    }

    #[test]
    fn a_picture_with_the_interface_always_asks() {
        assert!(always_asks("camera_snapshot", &json!({"interface": true})));
        assert!(!always_asks("camera_snapshot", &json!({"interface": false})));
        assert!(!always_asks("camera_snapshot", &json!({})));
        assert!(!always_asks("camera_preset", &json!({"interface": true})));
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
