//! Folderstorm: the file upload tools (folderstorm/plans/mcp-uploads.md),
//! after Folderstorm (C++)'s `tools/fs-mcp/internal/mcptools/uploads.go`
//! and the texture mode of its `inventory_snapshot_upload`, with their
//! names. Local files only, on the machine running the viewer: a preview
//! reads, hashes and prices them and keeps the bytes as a plan; the execute
//! uploads that plan once, after the user's click in the viewer whenever it
//! costs L$. Kept apart from the other catalogues; [`super::all`] lists
//! every group.

use super::{CHANGE_WAIT, SNAPSHOT_EDGE_MAX, SNAPSHOT_EDGE_MIN, SNAPSHOT_WAIT, ToolSpec};
use crate::policy::Class;
use serde_json::{Value, json};
use std::time::Duration;

pub const PREVIEW: &str = "inventory_upload_preview";
pub const EXECUTE: &str = "inventory_upload_execute";
pub const STATUS: &str = "inventory_upload_status";
pub const CANCEL: &str = "inventory_upload_cancel";
pub const MESH_REVIEW: &str = "inventory_upload_mesh_review";
pub const SNAPSHOT: &str = "inventory_snapshot_upload";

/// Files one plan may hold (Folderstorm (C++)'s 32).
pub const FILES_MAX: usize = 32;
/// The largest file, in bytes (64 MiB).
pub const FILE_BYTES_MAX: u64 = 64 << 20;
/// The bytes one plan may hold, and all the plans of a login together
/// (128 MiB).
pub const PLAN_BYTES_MAX: u64 = 128 << 20;
/// How deep a folder's files may sit below it.
pub const DEPTH_MAX: usize = 8;
/// New inventory folders one plan may create.
pub const FOLDERS_MAX: usize = 64;
/// Plans kept per login (the oldest unused one goes).
pub const PLANS_MAX: usize = 4;
/// How long a plan lasts, in seconds.
pub const PLAN_SECONDS: u64 = 120;
/// The texture sizes a preview may ask for (the uploader's 1K and 2K).
pub const TEXTURE_DIMENSIONS: [u32; 2] = [1024, 2048];
/// A path's longest form, in bytes.
pub const PATH_BYTES_MAX: usize = 4096;
/// How long a preview may read and decode its files.
pub const PREVIEW_WAIT: Duration = Duration::from_secs(60);
/// How long an execute may wait on top (its folders and files, one by one;
/// it answers early for a sidecar that declares `operations`).
pub const EXECUTE_WAIT: Duration = Duration::from_secs(600);

/// Whether `tool` belongs to this group.
pub fn handles(tool: &str) -> bool {
    UPLOAD_TOOLS.iter().any(|t| t.name == tool)
}

/// The execute needs its `client_request_id`: an upload cannot be looked
/// at afterwards (mcp-receipts.md, decision 3).
pub fn needs_client_request_id(tool: &str) -> bool {
    tool == EXECUTE
}

/// The execute answers as soon as the viewer took it (its files go one by
/// one, for minutes); the assistant then polls `inventory_upload_status`.
pub fn answers_early(tool: &str) -> bool {
    tool == EXECUTE
}

const PLAN_ID: &str = "The plan_id a preview (inventory_upload_preview or inventory_snapshot_upload) answered.";

fn folder_properties() -> Value {
    json!({
        "folder_id": {"type": "string", "description": "The destination inventory folder's UUID."},
        "path": {"type": "string", "description": "Instead of `folder_id`: the destination folder's path, e.g. \"Textures/Shop\"."}
    })
}

fn preview_schema() -> Value {
    let mut properties = folder_properties();
    properties["files"] = json!({"type": "array", "minItems": 1, "maxItems": FILES_MAX, "items": {"type": "string"},
        "description": "Absolute local paths of files on the machine running the viewer (PNG, JPEG, TGA, BMP, WAV, ANIM, BVH). Give `files` or `directory`."});
    properties["directory"] = json!({"type": "string",
        "description": "Instead of `files`: one absolute local folder, uploaded as a new inventory folder of the same name with its sub-folders (32 files, 8 levels at most)."});
    properties["max_texture_dimension"] = json!({"type": "integer", "enum": TEXTURE_DIMENSIONS, "default": TEXTURE_DIMENSIONS[0],
        "description": "The largest side a texture is uploaded at; larger images are scaled down. 2048 may cost more."});
    json!({"type": "object", "properties": properties, "additionalProperties": false})
}

fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"plan_id": {"type": "string", "description": PLAN_ID}},
        "required": ["plan_id"],
        "additionalProperties": false
    })
}

fn status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "plan_id": {"type": "string", "description": PLAN_ID},
            "operation_id": {"type": "string", "description": "Instead of `plan_id`: the execute's operation_id."}
        },
        "additionalProperties": false
    })
}

fn cancel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"operation_id": {"type": "string", "description": "The execute's operation_id."}},
        "required": ["operation_id"],
        "additionalProperties": false
    })
}

fn mesh_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"path": {"type": "string", "description": "The absolute local path of a Collada (.dae) file on the machine running the viewer."}},
        "required": ["path"],
        "additionalProperties": false
    })
}

fn snapshot_schema() -> Value {
    let edge = |what: &str| json!({"type": "integer", "minimum": SNAPSHOT_EDGE_MIN, "maximum": SNAPSHOT_EDGE_MAX, "description": what});
    let fraction = json!({"type": "number", "minimum": 0.0, "maximum": 1.0});
    let mut properties = folder_properties();
    properties["folder_id"] = json!({"type": "string", "description": "The folder the texture goes to (UUID). Default: the Photo Album, as the viewer's photos."});
    properties["name"] = json!({"type": "string", "description": "The new texture's name (1 to 63 characters, no `|`). Default: \"Snapshot\" and the date."});
    properties["thumbnail_id"] = json!({"type": "string",
        "description": "An outfit, folder or item (UUID) whose picture becomes this texture once it is uploaded; needs Edit too at the execute."});
    properties["source"] = json!({"type": "string", "enum": ["window", "world"], "default": "world",
        "description": "world: the world view only; window: the window's proportions. Never the viewer's interface."});
    properties["width"] = edge("The picture's width. With `height`: that size, centre-cropped. Alone: the height follows.");
    properties["height"] = edge("The picture's height.");
    properties["max_edge"] = json!({"type": "integer", "minimum": SNAPSHOT_EDGE_MIN, "maximum": SNAPSHOT_EDGE_MAX, "default": 1024,
        "description": "Without width and height: the long edge at most."});
    properties["crop"] = json!({"type": "object", "description": "The part of the source kept, in fractions of its width and height.",
        "properties": {"x": fraction, "y": fraction, "width": fraction, "height": fraction},
        "required": ["x", "y", "width", "height"], "additionalProperties": false});
    properties["huds"] = json!({"type": "boolean", "default": false, "description": "Include the HUDs."});
    json!({"type": "object", "properties": properties, "additionalProperties": false})
}

const fn read(name: &'static str, description: &'static str, schema: fn() -> Value, wait: Duration) -> ToolSpec {
    ToolSpec {
        name,
        description,
        class: Some(Class::Read),
        also: None,
        schema,
        group: crate::binding::UPLOADS,
        wait,
    }
}

/// The file upload tools.
pub const UPLOAD_TOOLS: &[ToolSpec] = &[
    read(
        PREVIEW,
        "Prepare an upload of local files (absolute paths on the machine running the viewer, never URLs): 1 to 32 PNG, JPEG, TGA, BMP, WAV, ANIM or BVH files, or one folder (its tree becomes a new inventory folder). Each file is read once, hashed (SHA-256), checked as the viewer's uploader checks it and priced at this account's upload fees; anything unsupported, linked or too large refuses the whole preview. Uploads and charges nothing. Answers a plan_id valid 120 seconds, the rows, the folders to create and the total in L$; then inventory_upload_execute. A .dae goes to inventory_upload_mesh_review.",
        preview_schema,
        PREVIEW_WAIT,
    ),
    ToolSpec {
        name: EXECUTE,
        description: "Upload a previewed plan once: its folders, then its files one by one, each at its quoted price, into your inventory. Anything above L$0 always asks the user in the viewer with the price; nothing is spent without that click. A higher price, a failure or an unclear answer stops the files not sent yet; never execute again or preview the same files again after an unclear answer: read inventory_upload_status. Needs a client_request_id. Answers at once with an operation_id. Never wears, plays or rezzes what it uploads.",
        class: Some(Class::Upload),
        also: Some(Class::Read),
        schema: plan_schema,
        group: crate::binding::UPLOADS,
        wait: EXECUTE_WAIT,
    },
    read(
        STATUS,
        "An upload's progress by plan_id or operation_id: each file's outcome (waiting, done with its new item id, failed, unknown, not_sent, cancelled), the folders created and the L$ spent. Never sends anything again.",
        status_schema,
        Duration::ZERO,
    ),
    ToolSpec {
        name: CANCEL,
        description: "Stop the files of an upload not sent yet. An upload already sent runs to its answer; accepted uploads and their charges cannot be undone. Works whatever the permissions.",
        class: None,
        also: None,
        schema: cancel_schema,
        group: crate::binding::UPLOADS,
        wait: Duration::ZERO,
    },
    ToolSpec {
        name: MESH_REVIEW,
        description: "Open one local Collada (.dae) file in the viewer's mesh uploader for the user to review (levels of detail, physics, rigging) and upload by hand. Uploads nothing and spends nothing: the uploader shows the server's fee and its own Upload button. Refused while the uploader is busy or already holds a mesh. Files the Collada file references beside it may be read too.",
        class: Some(Class::Upload),
        also: None,
        schema: mesh_schema,
        group: crate::binding::UPLOADS,
        wait: CHANGE_WAIT,
    },
    ToolSpec {
        name: SNAPSHOT,
        description: "Take a picture of what the camera sees (as camera_snapshot, never the interface) and prepare it as a new texture of your inventory: answers a plan_id valid 120 seconds with the texture's size and price. Uploads and charges nothing: inventory_upload_execute does, asking the user with the price. With thumbnail_id, that entry's picture becomes the new texture afterwards. Frame it first with camera_preset or camera_shot.",
        class: Some(Class::Camera),
        also: Some(Class::Read),
        schema: snapshot_schema,
        group: crate::binding::UPLOADS,
        wait: Duration::from_secs(SNAPSHOT_WAIT.as_secs() + PREVIEW_WAIT.as_secs()),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_tools_have_their_classes_and_bounded_schemas() {
        for t in UPLOAD_TOOLS {
            let schema = (t.schema)();
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false, "{}", t.name);
            assert!(super::super::tool(t.name).is_some(), "{} reachable", t.name);
            assert!(t.bound(), "{} bound to one login", t.name);
            assert!(handles(t.name));
            let required = t.input_schema()["required"].as_array().cloned().unwrap_or_default();
            assert_eq!(
                required.iter().filter(|r| *r == "client_request_id").count(),
                usize::from(needs_client_request_id(t.name)),
                "{}",
                t.name
            );
            assert_eq!(t.needs_client_request_id(), needs_client_request_id(t.name));
            // nothing asks on Allow by itself: a price does, from the plan
            assert!(!super::super::always_asks(t.name, &json!({})), "{}", t.name);
        }
        assert_eq!(UPLOAD_TOOLS.len(), 6);
        let execute = super::super::tool(EXECUTE).expect("execute");
        assert_eq!(execute.classes().collect::<Vec<_>>(), vec![Class::Upload, Class::Read]);
        assert!(execute.changes());
        let mesh = super::super::tool(MESH_REVIEW).expect("mesh");
        assert_eq!(mesh.classes().collect::<Vec<_>>(), vec![Class::Upload]);
        let snapshot = super::super::tool(SNAPSHOT).expect("snapshot");
        assert_eq!(snapshot.classes().collect::<Vec<_>>(), vec![Class::Camera, Class::Read]);
        assert!(snapshot.wait > SNAPSHOT_WAIT);
        for read in [PREVIEW, STATUS] {
            assert_eq!(super::super::tool(read).and_then(|t| t.class), Some(Class::Read), "{read}");
        }
        let cancel = super::super::tool(CANCEL).expect("cancel");
        assert!(cancel.class.is_none() && !cancel.changes());
        assert!(answers_early(EXECUTE) && !answers_early(PREVIEW));
        assert_eq!(preview_schema()["properties"]["files"]["maxItems"], FILES_MAX);
        assert!(snapshot_schema()["properties"].get("interface").is_none());
    }
}
