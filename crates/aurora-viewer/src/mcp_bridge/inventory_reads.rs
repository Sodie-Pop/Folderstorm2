//! Folderstorm: the reads of phase 3, part 2 (folderstorm/plans/mcp.md,
//! "Phase 3, part 2: inventory gaps"): several entries at once, system and
//! protected folders, the type names, an outfit's items and the extra
//! search filters. Pure functions of [`Ctx`], like `tools`.
//!
//! Creators' permissions: the entries are those of `tools` (an asset id only
//! for an item held with full permissions, never a thumbnail id); the
//! permission filters read `actions::held`, the rule the viewer applies.

use super::tools::{Ctx, err, folder_entry, item, item_entry, kind, path, scope, system_name, uuid_arg, wearable_name};
use crate::world::appearance;
use crate::world::inventory::{FetchState, actions};
use aurora_net::inventory::InvItem;
use folderstorm_mcp_bridge::protocol::inventory::{CREATE_TYPES, LINK_FILTERS};
use folderstorm_mcp_bridge::protocol::{CHANGE_MAX, KINDS, LIST_MAX};
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

/// The arguments of this group's reads, for `tools::answer`'s check.
pub fn known_argument(tool: &str, key: &str) -> Option<bool> {
    let known: &[&str] = match tool {
        "inventory_get_many" => &["ids"],
        "inventory_system_folder" => &["type"],
        "inventory_types" | "inventory_protected_folders" => &[],
        "appearance_outfit_items" => &["outfit_id", "path"],
        _ => return None,
    };
    Some(known.contains(&key))
}

/// Answer one of this group's reads, None for another tool.
pub fn answer(tool: &str, args: &Value, cx: &Ctx) -> Option<Result<Value, ToolError>> {
    Some(match tool {
        "inventory_get_many" => get_many(args, cx),
        "inventory_system_folder" => system_folder(args, cx),
        "inventory_types" => Ok(types()),
        "inventory_protected_folders" => Ok(protected_folders(cx)),
        "appearance_outfit_items" => outfit_items(args, cx),
        _ => return None,
    })
}

/// `inventory_get_many`: each id as `inventory_item` gives an item, a
/// folder as `inventory_list_folder` names it.
fn get_many(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let list = args["ids"]
        .as_array()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`ids` is required and must be a list of UUIDs"))?;
    if list.is_empty() || list.len() > CHANGE_MAX {
        return Err(err(ErrorCode::InvalidArguments, format!("`ids` must hold 1 to {CHANGE_MAX} ids")));
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut missing = Vec::new();
    for value in list {
        let id = value
            .as_str()
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .ok_or_else(|| err(ErrorCode::InvalidArguments, "`ids` holds something that is not a UUID"))?;
        if !seen.insert(id) {
            continue;
        }
        if cx.inv.folders.contains_key(&id) {
            let mut entry = folder_entry(cx, id);
            if let Some(f) = cx.inv.folders.get(&id) {
                entry["folders"] = json!(f.children.len());
                entry["items"] = json!(f.items.len());
            }
            entries.push(entry);
        } else if cx.inv.items.contains_key(&id) {
            entries.push(item(&json!({"item_id": id.to_string()}), cx)?);
        } else {
            missing.push(id);
        }
    }
    Ok(json!({"entries": entries, "missing": missing}))
}

/// The LLFolderType of a system folder name (`tools::system_name`), with
/// the spellings an assistant may use ("Body Parts", "Lost and Found").
fn folder_type(name: &str) -> Option<i32> {
    let wanted = name.trim().to_lowercase().replace([' ', '-'], "_");
    let wanted = match wanted.as_str() {
        "textures" | "texture" => "textures",
        "landmark" => "landmarks",
        "object" => "objects",
        "notecard" => "notecards",
        "script" => "scripts",
        "body_part" | "bodyparts" => "body_parts",
        "snapshot" | "photo_album" | "photos" => "snapshots",
        "animation" => "animations",
        "gesture" => "gestures",
        "favorite" => "favorites",
        "cof" => "current_outfit",
        "outfits" => "my_outfits",
        "inbox" => "received_items",
        "mesh" => "meshes",
        "material" => "materials",
        "sound" => "sounds",
        "calling_card" | "callingcards" => "calling_cards",
        "lost_found" => "lost_and_found",
        other => other,
    }
    .to_owned();
    // ordinary outfit folders (47) and the root are not one folder
    (0..=60).find(|kind| *kind != 47 && *kind != 8 && system_name(*kind) == Some(wanted.as_str()))
}

fn system_names() -> Vec<&'static str> {
    (0..=60).filter(|k| *k != 47 && *k != 8).filter_map(system_name).collect()
}

/// `inventory_system_folder` (LLInventoryModel::findCategoryUUIDForType):
/// My Inventory's folder of that type, never the Library's.
fn system_folder(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let name = args["type"]
        .as_str()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`type` is required and must be a string"))?;
    let kind = folder_type(name).ok_or_else(|| {
        err(
            ErrorCode::InvalidArguments,
            format!("`type` is one of: {}", system_names().join(", ")),
        )
    })?;
    let id = actions::system(cx.inv, kind).ok_or_else(|| err(ErrorCode::NotFound, "this inventory has no such folder (yet)"))?;
    Ok(folder_entry(cx, id))
}

fn types() -> Value {
    json!({
        "kinds": KINDS,
        "system_folders": system_names(),
        "wearables": (0..17).map(wearable_name).collect::<Vec<_>>(),
        "create_item_types": CREATE_TYPES,
        "search_link_filters": LINK_FILTERS,
    })
}

/// `inventory_protected_folders`: the folders the changes leave alone, with
/// the rule that holds each (`actions::movable`, `actions::pinned`, the
/// window's locks, the Marketplace guard of `changes`).
fn protected_folders(cx: &Ctx) -> Value {
    let inv = cx.inv;
    let marketplace = crate::world::marketplace::root(inv);
    let mut list: Vec<(String, Value)> = inv
        .folders
        .iter()
        .filter(|(_, f)| !f.library)
        .filter_map(|(id, f)| {
            let mut why = Vec::new();
            if *id == inv.root || !matches!(f.info.type_default, -1 | 47 | 54) {
                why.push("system");
            }
            if cx.protected.contains(id) {
                why.push("locked");
            }
            if actions::pinned(inv, *id) {
                why.push("pinned");
            }
            if marketplace.is_some_and(|m| actions::under(inv, *id, m)) {
                why.push("marketplace");
            }
            if inv.listing_holds.holds(inv, *id) {
                why.push("listing");
            }
            if why.is_empty() {
                return None;
            }
            let mut entry = folder_entry(cx, *id);
            entry["protected"] = json!(why);
            Some((path(inv, *id).to_lowercase(), entry))
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    let truncated = list.len() > LIST_MAX;
    let folders: Vec<Value> = list.into_iter().take(LIST_MAX).map(|(_, v)| v).collect();
    json!({
        "folders": folders,
        "truncated": truncated,
        "library_id": (!inv.lib_root.is_nil()).then_some(inv.lib_root),
        "library": "the Library is read only: copy what you need from it",
    })
}

/// `appearance_outfit_items` (LLOutfitsList's items, LLPanelWearableOutfitItem):
/// the outfit folder's entries, links followed to their originals, worn as
/// Appearance shows it.
fn outfit_items(args: &Value, cx: &Ctx) -> Result<Value, ToolError> {
    let inv = cx.inv;
    let id = uuid_arg(args, "outfit_id")?;
    let folder = match (id, args["path"].is_null()) {
        (Some(_), false) => return Err(err(ErrorCode::InvalidArguments, "give `outfit_id` or `path`, not both")),
        (Some(id), true) if inv.folders.contains_key(&id) => id,
        (Some(_), true) => return Err(err(ErrorCode::NotFound, "no such folder")),
        (None, false) => scope(&json!({"path": args["path"]}), inv)?.ok_or_else(|| err(ErrorCode::NotFound, "no such folder"))?,
        (None, true) => return Err(err(ErrorCode::InvalidArguments, "give the outfit as `outfit_id` or `path`")),
    };
    let f = inv.folders.get(&folder).ok_or_else(|| err(ErrorCode::NotFound, "no such folder"))?;
    let mut items: Vec<(String, Value)> = f
        .items
        .iter()
        .filter_map(|id| inv.items.get(id))
        .map(|entry| {
            let original = actions::original(inv, entry.id).and_then(|o| inv.items.get(&o));
            let mut out = item_entry(cx, entry);
            match original {
                Some(o) if o.id != entry.id => {
                    out["target_id"] = json!(o.id);
                    out["type"] = json!(kind(o));
                    out["worn"] = json!(cx.worn.contains(&o.id));
                    if matches!(o.asset_type, 5 | 13) {
                        out["wearable"] = json!(wearable_name(o.flags));
                    }
                }
                Some(_) => {}
                None => out["broken"] = json!(true),
            }
            if let Some(point) = original.and_then(|o| cx.attachment_points.get(&o.id)) {
                out["attachment_point"] = json!(point);
            }
            (entry.name.to_lowercase(), out)
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let truncated = items.len() > LIST_MAX;
    Ok(json!({
        "outfit_id": folder,
        "name": f.info.name,
        "fetched": f.state == FetchState::Fetched,
        "current_base": appearance::base(inv) == Some(folder),
        "items": items.into_iter().take(LIST_MAX).map(|(_, v)| v).collect::<Vec<_>>(),
        "truncated": truncated,
    }))
}

/// `inventory_search`'s filters of phase 3 (Folderstorm (C++)'s
/// `inventory_search` fields).
#[derive(Debug, Default)]
pub struct Filters {
    description: Option<String>,
    creator: Option<Uuid>,
    worn: Option<bool>,
    copyable: Option<bool>,
    modifiable: Option<bool>,
    favorite: Option<bool>,
    created_after: Option<i64>,
    created_before: Option<i64>,
    links: Links,
    linked: Option<Uuid>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Links {
    #[default]
    Include,
    Exclude,
    Only,
}

fn flag(args: &Value, key: &str) -> Result<Option<bool>, ToolError> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        _ => Err(err(ErrorCode::InvalidArguments, format!("`{key}` must be true or false"))),
    }
}

fn time(args: &Value, key: &str) -> Result<Option<i64>, ToolError> {
    match &args[key] {
        Value::Null => Ok(None),
        v => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| err(ErrorCode::InvalidArguments, format!("`{key}` must be a whole number of seconds"))),
    }
}

impl Filters {
    pub fn parse(args: &Value) -> Result<Filters, ToolError> {
        let description = match &args["description"] {
            Value::Null => None,
            Value::String(s) if s.trim().is_empty() => None,
            Value::String(s) => Some(s.trim().to_lowercase()),
            _ => return Err(err(ErrorCode::InvalidArguments, "`description` must be a string")),
        };
        let links = match args["filter_links"].as_str().map(|s| s.to_lowercase()) {
            None if args["filter_links"].is_null() => Links::Include,
            // Folderstorm (C++)'s spellings too
            Some(s) if s == "include" || s == "include_links" => Links::Include,
            Some(s) if s == "exclude" || s == "exclude_links" => Links::Exclude,
            Some(s) if s == "only" || s == "only_links" => Links::Only,
            _ => return Err(err(ErrorCode::InvalidArguments, "`filter_links` is `include`, `exclude` or `only`")),
        };
        Ok(Filters {
            description,
            creator: uuid_arg(args, "creator_id")?,
            worn: flag(args, "worn")?,
            copyable: flag(args, "copyable")?,
            modifiable: flag(args, "modifiable")?,
            favorite: flag(args, "favorite")?,
            created_after: time(args, "created_after")?,
            created_before: time(args, "created_before")?,
            links,
            linked: uuid_arg(args, "linked_id")?,
        })
    }

    /// Whether only items can match (a filter folders do not have).
    pub fn items_only(&self) -> bool {
        self.description.is_some()
            || self.creator.is_some()
            || self.worn.is_some()
            || self.copyable.is_some()
            || self.modifiable.is_some()
            || self.created_after.is_some()
            || self.created_before.is_some()
            || self.links == Links::Only
            || self.linked.is_some()
    }

    pub fn folder_ok(&self, cx: &Ctx, folder: Uuid) -> bool {
        !self.items_only() && self.favorite.is_none_or(|f| actions::is_favorite(cx.inv, folder) == f)
    }

    /// An item: a link is judged by its original (its rights, its creator,
    /// whether it is worn), as the inventory's filters judge it.
    pub fn item_ok(&self, cx: &Ctx, item: &InvItem) -> bool {
        let inv = cx.inv;
        let link = matches!(item.asset_type, 24 | 25);
        match self.links {
            Links::Exclude if link => return false,
            Links::Only if !link => return false,
            _ => {}
        }
        if self.linked.is_some_and(|target| !link || item.asset_id != target) {
            return false;
        }
        let original = actions::original(inv, item.id).and_then(|id| inv.items.get(&id)).unwrap_or(item);
        let held = actions::held(inv, original);
        self.description.as_ref().is_none_or(|d| item.desc.to_lowercase().contains(d))
            && self.creator.is_none_or(|c| original.creator == c)
            && self.worn.is_none_or(|w| cx.worn.contains(&original.id) == w)
            && self.copyable.is_none_or(|c| (held & actions::COPY != 0) == c)
            && self.modifiable.is_none_or(|m| (held & actions::MODIFY != 0) == m)
            && self.favorite.is_none_or(|f| actions::is_favorite(inv, item.id) == f)
            && self.created_after.is_none_or(|t| original.created_at >= t)
            && self.created_before.is_none_or(|t| original.created_at < t)
    }
}
