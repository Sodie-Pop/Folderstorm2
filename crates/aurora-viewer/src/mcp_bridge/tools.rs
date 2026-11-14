//! Folderstorm: the answers of the local assistant's read-only tools
//! (folderstorm/plans/mcp.md), computed on the frame thread from the
//! inventory model and the worn items. Pure functions of [`Ctx`], so the
//! tests run them on the demo inventory.
//!
//! Creators' permissions: an item's asset id is given only when the viewer
//! would show it (`actions::held` with `actions::FULL`, the rule of
//! LLViewerInventoryItem::getProtectedAssetUUID, indra/newview/
//! llviewerinventory.cpp, that `actions::asset_uuid_shown` applies).
//! Stricter than the Legacy Viewer: no demo exception, no thumbnails, and
//! RLVa's @showinv refuses every inventory read.

use crate::world::appearance;
use crate::world::inventory::{FetchState, Inventory, actions};
use aurora_net::inventory::InvItem;
use folderstorm_mcp_bridge::protocol::{KINDS, LIST_MAX, SEARCH_DEFAULT, SEARCH_MAX};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// LLFolderType::FT_TRASH.
const FT_TRASH: i32 = 14;
/// Folders a path may match at one level before it is refused as too
/// ambiguous to follow.
const PATH_BRANCHES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    LoggedOut,
    LoggedIn,
    Demo,
}

/// What the tools read, gathered by the frame thread.
pub struct Ctx<'a> {
    pub inv: &'a Inventory,
    pub session: Session,
    /// Worn clothing and body parts (Current Outfit links) and attached
    /// objects, by inventory item id.
    pub worn: HashSet<Uuid>,
    /// Attachment point of each attached object's item.
    pub attachment_points: HashMap<Uuid, u8>,
    /// RLVa's @showinv: the inventory may not be looked at.
    pub inventory_locked: bool,
    /// The folders locked in the inventory window (`InventoryPreferences::
    /// protected`), which the changes leave in place as the window does.
    pub protected: HashSet<Uuid>,
    /// The worn objects' points, temporary attachments left out
    /// (`World::outfit_attachment_points`), for the outfit rules.
    pub outfit_points: HashMap<Uuid, u8>,
    /// What the camera tools read (phase 3).
    pub stage: super::camera::Stage,
    /// The login's `viewer_session` (None while logged out), which
    /// `viewer_status` gives.
    pub viewer_session: Option<String>,
    /// Session routing: the profile, managed or not, and the character's
    /// name (only while Read is Allow), which `viewer_status` gives.
    pub identity: folderstorm_mcp_bridge::server::Identity,
    /// Folderstorm: what the navigation tools read (phase 3, part 4).
    pub nav: super::navigation::Stage<'a>,
}

pub(super) fn err(code: ErrorCode, message: impl Into<String>) -> ToolError {
    ToolError::new(code, message)
}

/// Answer one call.
pub fn answer(tool: &str, args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    if tool == "viewer_status" {
        return Ok(status(cx));
    }
    if args.as_object().is_none_or(|a| a.keys().any(|k| !known_argument(tool, k))) {
        return Err(err(ErrorCode::InvalidArguments, "unknown argument"));
    }
    ready(cx)?;
    match tool {
        "inventory_search" => search(args, cx),
        "inventory_list_folder" => list_folder(args, cx),
        "inventory_item" => item(args, cx),
        "outfits_list" => Ok(outfits(cx)),
        "worn_items" => Ok(worn(cx)),
        // Folderstorm: phase 3, part 2 (inventory gaps)
        _ => super::inventory_reads::answer(tool, args, cx).unwrap_or_else(|| Err(err(ErrorCode::UnknownTool, "unknown tool"))),
    }
}

fn known_argument(tool: &str, key: &str) -> bool {
    let known: &[&str] = match tool {
        "inventory_search" => {
            return ["query", "folder_id", "path", "type", "include_library", "include_trash", "limit"].contains(&key)
                || folderstorm_mcp_bridge::protocol::inventory::SEARCH_FILTERS.contains(&key);
        }
        "inventory_list_folder" => &["folder_id", "path"],
        "inventory_item" => &["item_id"],
        _ => return super::inventory_reads::known_argument(tool, key).unwrap_or(false),
    };
    known.contains(&key)
}

pub(super) fn ready(cx: &Ctx) -> Result<(), ToolError> {
    if cx.session == Session::LoggedOut || cx.inv.root.is_nil() {
        return Err(err(ErrorCode::NotReady, "the viewer is not logged in"));
    }
    if cx.inventory_locked {
        return Err(err(
            ErrorCode::PermissionDenied,
            "the inventory is locked by an RLV restriction (@showinv)",
        ));
    }
    Ok(())
}

fn status(cx: &Ctx) -> Value {
    let inv = cx.inv;
    let total = inv.folders.len();
    let fetched = inv.folders.values().filter(|f| f.state == FetchState::Fetched).count();
    let usable = cx.session != Session::LoggedOut && !inv.root.is_nil() && inv.folders.contains_key(&inv.root);
    json!({
        "product": folderstorm_brand::PRODUCT,
        "version": aurora_version::version(),
        "session": match cx.session {
            Session::LoggedOut => "logged_out",
            Session::LoggedIn => "logged_in",
            Session::Demo => "demo",
        },
        // the login the calls are bound to, and what this viewer implements
        // (folderstorm/plans/mcp-receipts.md, sections 2 and 3)
        "viewer_session": cx.viewer_session,
        "capabilities": folderstorm_mcp_bridge::binding::capabilities(),
        // which character this is (folderstorm/plans/mcp-routing.md)
        "profile": cx.identity.profile.as_deref().unwrap_or("main"),
        "managed": cx.identity.managed,
        "character": cx.identity.character,
        "inventory": {
            "usable": usable && !cx.inventory_locked,
            "locked_by_rlv": cx.inventory_locked,
            "folders_fetched": fetched,
            "folders_total": total,
            "items_known": inv.items.len(),
            "complete": usable && fetched == total,
        }
    })
}

/// The kind of an item, from its asset and inventory types
/// (LLAssetType / LLInventoryType).
pub(super) fn kind(item: &InvItem) -> &'static str {
    match item.asset_type {
        0 if item.inv_type == 15 => "snapshot",
        0 => "texture",
        1 => "sound",
        2 => "calling_card",
        3 => "landmark",
        5 => "clothing",
        6 => "object",
        7 => "notecard",
        10 => "script",
        13 => "body_part",
        20 => "animation",
        21 => "gesture",
        24 => "link",
        25 => "folder_link",
        56 => "settings",
        57 => "material",
        _ => "other",
    }
}

/// The name of a system folder type (LLFolderType), None for an ordinary
/// folder.
pub(super) fn system_name(kind: i32) -> Option<&'static str> {
    Some(match kind {
        0 => "textures",
        1 => "sounds",
        2 => "calling_cards",
        3 => "landmarks",
        5 => "clothing",
        6 => "objects",
        7 => "notecards",
        8 => "root",
        10 => "scripts",
        13 => "body_parts",
        14 => "trash",
        15 => "snapshots",
        16 => "lost_and_found",
        20 => "animations",
        21 => "gestures",
        23 => "favorites",
        46 => "current_outfit",
        47 => "outfit",
        48 => "my_outfits",
        49 => "meshes",
        50 => "received_items",
        51 => "outbox",
        53 => "marketplace_listings",
        56 => "settings",
        57 => "materials",
        _ => return None,
    })
}

/// The wearable type in an item's flags (LLWearableType).
pub(super) fn wearable_name(flags: u32) -> &'static str {
    match flags & 0xff {
        0 => "shape",
        1 => "skin",
        2 => "hair",
        3 => "eyes",
        4 => "shirt",
        5 => "pants",
        6 => "shoes",
        7 => "socks",
        8 => "jacket",
        9 => "gloves",
        10 => "undershirt",
        11 => "underpants",
        12 => "skirt",
        13 => "alpha",
        14 => "tattoo",
        15 => "physics",
        16 => "universal",
        _ => "unknown",
    }
}

/// A folder's path: names from My Inventory (empty for the root itself),
/// or from "Library".
pub(super) fn path(inv: &Inventory, folder: Uuid) -> String {
    let mut names: Vec<&str> = Vec::new();
    for id in actions::ancestors(inv, folder) {
        if id == inv.root {
            break;
        }
        if id == inv.lib_root {
            names.push("Library");
            break;
        }
        match inv.folders.get(&id) {
            Some(f) => names.push(&f.info.name),
            None => break,
        }
    }
    names.reverse();
    names.join("/")
}

pub(super) fn uuid_arg(args: &Value, key: &str) -> Result<Option<Uuid>, ToolError> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Uuid::parse_str(s.trim())
            .map(Some)
            .map_err(|_| err(ErrorCode::InvalidArguments, format!("`{key}` is not a UUID"))),
        _ => Err(err(ErrorCode::InvalidArguments, format!("`{key}` must be a string"))),
    }
}

fn bool_arg(args: &Value, key: &str) -> Result<bool, ToolError> {
    match &args[key] {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(*b),
        _ => Err(err(ErrorCode::InvalidArguments, format!("`{key}` must be true or false"))),
    }
}

/// Follow a `/` path from My Inventory, or from "Library/". A level
/// matches by exact name first, then ignoring case.
pub(super) fn resolve_path(inv: &Inventory, text: &str) -> Result<Uuid, ToolError> {
    let mut parts: Vec<&str> = text.split('/').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut current = vec![inv.root];
    let root_name = inv.folders.get(&inv.root).map(|f| f.info.name.as_str());
    if let Some(first) = parts.first().copied() {
        if first.eq_ignore_ascii_case("Library") && !inv.lib_root.is_nil() && inv.folders.contains_key(&inv.lib_root) {
            current = vec![inv.lib_root];
            parts.remove(0);
        } else if root_name.is_some_and(|name| name == first) {
            parts.remove(0);
        }
    }
    for part in parts {
        let children = |exact: bool| -> Vec<Uuid> {
            current
                .iter()
                .filter_map(|id| inv.folders.get(id))
                .flat_map(|f| f.children.iter())
                .filter(|child| {
                    inv.folders.get(child).is_some_and(|c| {
                        if exact {
                            c.info.name == part
                        } else {
                            c.info.name.to_lowercase() == part.to_lowercase()
                        }
                    })
                })
                .copied()
                .collect()
        };
        let mut next = children(true);
        if next.is_empty() {
            next = children(false);
        }
        if next.is_empty() {
            return Err(err(ErrorCode::NotFound, format!("no folder named `{part}` there")));
        }
        if next.len() > PATH_BRANCHES {
            return Err(err(ErrorCode::Ambiguous, "the path matches too many folders; use a folder id"));
        }
        current = next;
    }
    match current.as_slice() {
        [one] => Ok(*one),
        many => Err(ToolError::new(
            ErrorCode::Ambiguous,
            format!(
                "the path names {} folders: {}",
                many.len(),
                many.iter().map(Uuid::to_string).collect::<Vec<_>>().join(", ")
            ),
        )),
    }
}

/// The folder named by `folder_id` or `path`; None when neither is given.
pub(super) fn scope(args: &Value, inv: &Inventory) -> Result<Option<Uuid>, ToolError> {
    let id = uuid_arg(args, "folder_id")?;
    let path = match &args["path"] {
        Value::Null => None,
        Value::String(s) => Some(s.as_str()),
        _ => return Err(err(ErrorCode::InvalidArguments, "`path` must be a string")),
    };
    match (id, path) {
        (Some(_), Some(_)) => Err(err(ErrorCode::InvalidArguments, "give `folder_id` or `path`, not both")),
        (Some(id), None) if inv.folders.contains_key(&id) => Ok(Some(id)),
        (Some(_), None) => Err(err(ErrorCode::NotFound, "no such folder")),
        (None, Some(path)) => resolve_path(inv, path).map(Some),
        (None, None) => Ok(None),
    }
}

pub(super) fn library(inv: &Inventory, folder: Uuid) -> bool {
    inv.folders.get(&folder).is_some_and(|f| f.library)
}

pub(super) fn item_entry(cx: &Ctx, item: &InvItem) -> Value {
    let mut entry = json!({
        "id": item.id,
        "name": item.name,
        "type": kind(item),
        "folder_id": item.parent,
        "path": path(cx.inv, item.parent),
        "worn": cx.worn.contains(&item.id) || (matches!(item.asset_type, 24 | 25) && cx.worn.contains(&item.asset_id)),
    });
    if matches!(item.asset_type, 5 | 13) {
        entry["wearable"] = json!(wearable_name(item.flags));
    }
    entry["favorite"] = json!(actions::is_favorite(cx.inv, item.id)); // Folderstorm: phase 3
    entry
}

pub(super) fn folder_entry(cx: &Ctx, folder: Uuid) -> Value {
    let Some(f) = cx.inv.folders.get(&folder) else {
        return json!({"id": folder, "type": "folder"});
    };
    let mut entry = json!({
        "id": folder,
        "name": f.info.name,
        "type": "folder",
        "folder_id": f.info.parent,
        "path": path(cx.inv, folder),
        "fetched": f.state == FetchState::Fetched,
    });
    if let Some(system) = system_name(f.info.type_default) {
        entry["system"] = json!(system);
    }
    entry["favorite"] = json!(f.info.favorite); // Folderstorm: phase 3
    entry
}

fn search(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let inv = cx.inv;
    let query = match &args["query"] {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_lowercase(),
        _ => return Err(err(ErrorCode::InvalidArguments, "`query` must be a string")),
    };
    let wanted = match &args["type"] {
        Value::Null => None,
        Value::String(s) if KINDS.contains(&s.as_str()) => Some(s.as_str()),
        _ => return Err(err(ErrorCode::InvalidArguments, "`type` is not a known kind")),
    };
    let limit = match &args["limit"] {
        Value::Null => SEARCH_DEFAULT,
        v => v
            .as_u64()
            .filter(|n| (1..=SEARCH_MAX as u64).contains(n))
            .ok_or_else(|| err(ErrorCode::InvalidArguments, format!("`limit` must be 1 to {SEARCH_MAX}")))? as usize,
    };
    let scope = scope(args, inv)?;
    let filters = super::inventory_reads::Filters::parse(args)?; // Folderstorm: phase 3
    let with_library = bool_arg(args, "include_library")? || scope.is_some_and(|s| library(inv, s));
    let with_trash = bool_arg(args, "include_trash")? || scope.is_some_and(|s| inv.in_folder_type(s, FT_TRASH));
    let place_ok = |id: Uuid, parent: Uuid| {
        (with_library || !library(inv, parent))
            && (with_trash || !inv.in_folder_type(id, FT_TRASH))
            && scope.is_none_or(|s| id != s && actions::under(inv, id, s))
    };
    let name_ok = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
    let mut found: Vec<(String, Uuid, bool)> = Vec::new();
    if wanted.is_none_or(|w| w == "folder") {
        for (id, f) in &inv.folders {
            if *id != inv.root && *id != inv.lib_root && name_ok(&f.info.name) && place_ok(*id, *id) && filters.folder_ok(cx, *id) {
                found.push((f.info.name.to_lowercase(), *id, true));
            }
        }
    }
    if wanted.is_none_or(|w| w != "folder") {
        for item in inv.items.values() {
            if wanted.is_none_or(|w| w == kind(item)) && name_ok(&item.name) && place_ok(item.id, item.parent) && filters.item_ok(cx, item)
            {
                found.push((item.name.to_lowercase(), item.id, false));
            }
        }
    }
    found.sort();
    let truncated = found.len() > limit;
    let results: Vec<Value> = found
        .iter()
        .take(limit)
        .filter_map(|(_, id, folder)| {
            if *folder {
                Some(folder_entry(cx, *id))
            } else {
                inv.items.get(id).map(|it| item_entry(cx, it))
            }
        })
        .collect();
    Ok(json!({"results": results, "total": found.len(), "truncated": truncated}))
}

fn list_folder(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let inv = cx.inv;
    let id = scope(args, inv)?.unwrap_or(inv.root);
    let f = inv.folders.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "no such folder"))?;
    let folders: Vec<Value> = f
        .children
        .iter()
        .filter(|c| inv.folders.contains_key(c))
        .take(LIST_MAX)
        .map(|c| folder_entry(cx, *c))
        .collect();
    let room = LIST_MAX - folders.len();
    let items: Vec<Value> = f
        .items
        .iter()
        .filter_map(|i| inv.items.get(i))
        .take(room)
        .map(|it| item_entry(cx, it))
        .collect();
    let truncated = f.children.len() + f.items.len() > LIST_MAX;
    Ok(json!({
        "folder": folder_entry(cx, id),
        "folders": folders,
        "items": items,
        "truncated": truncated,
    }))
}

fn rights(mask: u32) -> Value {
    json!({
        "copy": mask & actions::COPY != 0,
        "modify": mask & actions::MODIFY != 0,
        "transfer": mask & actions::TRANSFER != 0,
    })
}

pub(super) fn item(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let inv = cx.inv;
    let id = uuid_arg(args, "item_id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "`item_id` is required"))?;
    if inv.folders.contains_key(&id) {
        return Err(err(ErrorCode::InvalidArguments, "that id is a folder: use inventory_list_folder"));
    }
    let item = inv.items.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "no such item"))?;
    let held = actions::held(inv, item);
    let mut out = item_entry(cx, item);
    out["description"] = json!(item.desc);
    out["creator_id"] = json!(item.creator);
    out["created"] = json!(item.created_at);
    out["library"] = json!(library(inv, item.parent));
    out["in_trash"] = json!(inv.in_folder_type(item.id, FT_TRASH));
    out["permissions"] = json!({"you": rights(held), "next_owner": rights(item.next_owner_mask)});
    match item.asset_type {
        // a link's "asset" is the item or folder it points to, not an asset
        24 => {
            let target = inv.items.get(&item.asset_id);
            out["link"] = json!({
                "target_id": item.asset_id,
                "target_name": target.map(|t| t.name.as_str()),
                "target_type": target.map(kind),
                "broken": target.is_none() && inv.broken(&item.asset_id),
            });
        }
        25 => {
            let target = inv.folders.get(&item.asset_id);
            out["link"] = json!({
                "target_id": item.asset_id,
                "target_name": target.map(|t| t.info.name.as_str()),
                "target_type": "folder",
            });
        }
        _ => {
            // LLViewerInventoryItem::getProtectedAssetUUID: full permissions only
            let shown = held & actions::FULL == actions::FULL;
            if shown && !item.asset_id.is_nil() {
                out["asset_id"] = json!(item.asset_id);
            }
            out["asset_id_hidden"] = json!(!shown && !item.asset_id.is_nil());
        }
    }
    if let Some(point) = cx.attachment_points.get(&item.id) {
        out["attachment_point"] = json!(point);
    }
    Ok(out)
}

fn outfits(cx: &Ctx) -> Value {
    let inv = cx.inv;
    let Some(my) = appearance::system_folder(inv, appearance::FT_MY_OUTFITS) else {
        return json!({"outfits": [], "my_outfits_id": null});
    };
    let base = appearance::base(inv);
    let mut list: Vec<(String, Value)> = inv
        .folders
        .get(&my)
        .into_iter()
        .flat_map(|f| f.children.iter())
        .filter_map(|id| inv.folders.get(id).map(|f| (id, f)))
        .map(|(id, f)| {
            (
                f.info.name.to_lowercase(),
                json!({
                    "id": id,
                    "name": f.info.name,
                    "items": f.items.len(),
                    "fetched": f.state == FetchState::Fetched,
                    "current_base": base == Some(*id),
                }),
            )
        })
        .take(LIST_MAX)
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    json!({"my_outfits_id": my, "outfits": list.into_iter().map(|(_, v)| v).collect::<Vec<_>>()})
}

fn worn(cx: &Ctx) -> Value {
    let mut entries: Vec<(u8, String, Value)> = cx
        .worn
        .iter()
        .map(|id| match cx.inv.items.get(id) {
            Some(item) => {
                let mut entry = item_entry(cx, item);
                if let Some(point) = cx.attachment_points.get(id) {
                    entry["attachment_point"] = json!(point);
                }
                let order = match item.asset_type {
                    13 => 0,
                    5 => 1,
                    _ => 2,
                };
                (order, item.name.to_lowercase(), entry)
            }
            // an attachment whose item is not loaded yet
            None => (3, String::new(), json!({"id": id, "name": null, "type": "object", "worn": true})),
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    json!({"worn": entries.into_iter().map(|(_, _, v)| v).take(LIST_MAX).collect::<Vec<_>>()})
}
