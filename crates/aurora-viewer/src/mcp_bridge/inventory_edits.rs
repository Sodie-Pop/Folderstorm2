//! Folderstorm: the changes of phase 3, part 2 (folderstorm/plans/mcp.md,
//! "Phase 3, part 2: inventory gaps"), planned with the rules of the
//! inventory window's own commands, like `changes`:
//!
//! - `inventory_create_item`: the New menu (`InvAction::Create`, where
//!   `context_catalog::creation` offers it: `actions::destination`, not
//!   the Trash nor Lost And Found);
//! - `inventory_set_favorite`: Add to / Remove from Favorites
//!   (`actions::set_favorites`, enabled as the menu's `favorites_ready`);
//! - `inventory_batch_rename`: Rename (`actions::renameable`,
//!   `actions::patch`) for each entry, all in one change;
//! - `inventory_set_thumbnail`: the Image window (LLFloaterChangeItemThumbnail):
//!   "Choose image" with a texture of the inventory (`thumbnail::usable`:
//!   Copy and Transfer, or the Library; never from the Trash), run through
//!   the window's own image path, and "Remove image". Stricter than the
//!   window: an item needs Modify.
//!
//! Nothing here sends anything: the app runs the work
//! (`app/local_assistant.rs`).

use super::changes::{Planned, Work};
use super::tools::{Ctx, err, path, scope, uuid_arg};
use crate::world::inventory::{Inventory, actions};
use aurora_net::inventory::operations::{Mutation, NewItem, metadata_thumbnail};
use folderstorm_mcp_bridge::protocol::CHANGE_MAX;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

/// LLFolderType::FT_TRASH and FT_LOST_AND_FOUND.
const FT_TRASH: i32 = 14;
const FT_LOST_AND_FOUND: i32 = 16;

/// A new item, as the New menu sends it (`InvAction::Create`).
#[derive(Debug, Clone, PartialEq)]
pub struct Creation {
    pub parent: Uuid,
    pub kind: NewItem,
    /// The landmark's name is the parcel's, chosen by the app when it is
    /// created; the other kinds always have one.
    pub name: Option<String>,
}

impl Creation {
    /// The asset type the created item arrives with.
    pub fn asset_type(&self) -> i32 {
        i32::from(self.kind.types().0)
    }
}

fn not_allowed(message: impl Into<String>) -> ToolError {
    err(ErrorCode::NotAllowed, message)
}

fn known(args: &Value, keys: &[&str]) -> Result<(), ToolError> {
    match args.as_object() {
        Some(a) if a.keys().all(|k| keys.contains(&k.as_str())) => Ok(()),
        _ => Err(err(ErrorCode::InvalidArguments, "unknown argument")),
    }
}

/// The Marketplace Listings folder and what it holds stay out of reach, as
/// in `changes`.
fn marketplace_guard(inv: &Inventory, ids: impl IntoIterator<Item = Uuid>) -> Result<(), ToolError> {
    if let Some(root) = crate::world::marketplace::root(inv)
        && ids.into_iter().any(|id| actions::under(inv, id, root))
    {
        return Err(not_allowed("the Marketplace Listings folder is not changed by an assistant"));
    }
    Ok(())
}

fn folder_label(inv: &Inventory, folder: Uuid) -> String {
    let path = path(inv, folder);
    if path.is_empty() { actions::name(inv, folder) } else { path }
}

fn what(inv: &Inventory, ids: &[Uuid]) -> String {
    let first = ids.first().map(|id| actions::name(inv, *id)).unwrap_or_default();
    if ids.len() == 1 {
        aurora_i18n::tr!("fs-mcp-ask-one", name = first)
    } else {
        aurora_i18n::tr!("fs-mcp-ask-many", count = ids.len(), name = first)
    }
}

fn nothing(reply: Value) -> Planned {
    Planned {
        work: Work::Nothing,
        summary: String::new(),
        reply,
        created: None,
    }
}

fn inventory(change: Mutation, summary: String, reply: Value) -> Planned {
    Planned {
        work: Work::Inventory(change),
        summary,
        reply,
        created: None,
    }
}

/// Plan one of this group's changes; None for another tool.
pub fn plan(tool: &str, args: &Value, cx: &Ctx) -> Option<Result<Planned, ToolError>> {
    Some(match tool {
        "inventory_create_item" => create_item(args, cx),
        "inventory_set_favorite" => set_favorite(args, cx),
        "inventory_batch_rename" => batch_rename(args, cx),
        "inventory_set_thumbnail" => set_thumbnail(args, cx),
        _ => return None,
    })
}

/// The kind of item a `type` names, its folder type, the New menu's label
/// (the item's default name) and the dialog's word for it.
fn new_kind(name: &str) -> Option<(NewItem, i32, &'static str, &'static str)> {
    Some(match name {
        "notecard" => (
            NewItem::Note,
            7,
            aurora_i18n::tr!("common-new-notecard"),
            aurora_i18n::tr!("fs-mcp-new-notecard"),
        ),
        "script" | "lsl" => (
            NewItem::Script,
            10,
            aurora_i18n::tr!("common-new-script"),
            aurora_i18n::tr!("fs-mcp-new-script"),
        ),
        "gesture" => (
            NewItem::Gesture,
            21,
            aurora_i18n::tr!("ui-inventory-context-new-gesture"),
            aurora_i18n::tr!("fs-mcp-new-gesture"),
        ),
        "material" => (
            NewItem::Material,
            57,
            aurora_i18n::tr!("ui-inventory-context-new-material"),
            aurora_i18n::tr!("fs-mcp-new-material"),
        ),
        "sky" => (
            NewItem::Settings(0),
            56,
            aurora_i18n::tr!("ui-inventory-context-new-sky"),
            aurora_i18n::tr!("fs-mcp-new-sky"),
        ),
        "water" => (
            NewItem::Settings(1),
            56,
            aurora_i18n::tr!("ui-inventory-context-new-water"),
            aurora_i18n::tr!("fs-mcp-new-water"),
        ),
        "day_cycle" | "daycycle" => (
            NewItem::Settings(2),
            56,
            aurora_i18n::tr!("ui-inventory-context-new-day-cycle"),
            aurora_i18n::tr!("fs-mcp-new-day-cycle"),
        ),
        "landmark" => (NewItem::Landmark, 3, "", ""),
        _ => return None,
    })
}

/// New Script, New Notecard… of the inventory's menu: in the folder given,
/// else the folder of the item's type (LLInventoryModel::findCategoryUUIDForType),
/// else My Inventory.
fn create_item(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["type", "name", "folder_id", "path"])?;
    let inv = cx.inv;
    let kind_name = args["type"]
        .as_str()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`type` is required and must be a string"))?;
    let (kind, folder_type, default_name, word) = new_kind(kind_name.trim()).ok_or_else(|| {
        err(
            ErrorCode::InvalidArguments,
            "`type` is notecard, script, gesture, material, sky, water, day_cycle or landmark (clothing and body parts upload an asset: not here)",
        )
    })?;
    let parent = match scope(args, inv)? {
        Some(parent) => parent,
        None => actions::system(inv, folder_type).unwrap_or(inv.root),
    };
    marketplace_guard(inv, [parent])?;
    if !actions::destination(inv, parent)
        || actions::in_type(inv, parent, FT_TRASH)
        || inv.folders.get(&parent).is_some_and(|f| f.info.type_default == FT_LOST_AND_FOUND)
    {
        return Err(not_allowed(
            "nothing can be created in this folder (the Library, Current Outfit, Received Items, Trash, My Outfits and Lost And Found take nothing new)",
        ));
    }
    let name = match &args["name"] {
        Value::Null => None,
        Value::String(s) => {
            let name = actions::clean_name(s).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
            crate::text_validation::require(crate::text_validation::Policy::PrintableNoPipe, &name)
                .map_err(|e| err(ErrorCode::InvalidArguments, e))?;
            Some(name)
        }
        _ => return Err(err(ErrorCode::InvalidArguments, "`name` must be a string")),
    };
    let folder = folder_label(inv, parent);
    let landmark = kind == NewItem::Landmark;
    let name = if landmark {
        name
    } else {
        name.or_else(|| Some(default_name.to_owned()))
    };
    let summary = match &name {
        _ if landmark => aurora_i18n::tr!("fs-mcp-ask-create-landmark", folder = folder),
        Some(name) => aurora_i18n::tr!("fs-mcp-ask-create-item", kind = word, name = name.clone(), folder = folder),
        None => String::new(),
    };
    let reply = json!({"type": kind_name.trim(), "name": name, "folder_id": parent});
    Ok(Planned {
        work: Work::Create(Creation { parent, kind, name }),
        summary,
        reply,
        created: None,
    })
}

fn ids_arg(args: &Value, inv: &Inventory) -> Result<Vec<Uuid>, ToolError> {
    let list = args["ids"]
        .as_array()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`ids` is required and must be a list of UUIDs"))?;
    if list.is_empty() || list.len() > CHANGE_MAX {
        return Err(err(ErrorCode::InvalidArguments, format!("`ids` must hold 1 to {CHANGE_MAX} ids")));
    }
    let mut ids = Vec::with_capacity(list.len());
    for value in list {
        let id = value
            .as_str()
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .ok_or_else(|| err(ErrorCode::InvalidArguments, "`ids` holds something that is not a UUID"))?;
        if !inv.items.contains_key(&id) && !inv.folders.contains_key(&id) {
            return Err(err(ErrorCode::NotFound, format!("no inventory entry {id}")));
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Add to / Remove from Favorites: the menu offers it outside the Trash,
/// for entries whose original is neither in the Library nor My Inventory
/// itself (`favorites_ready` of `context_catalog`); a link marks its
/// original (`actions::set_favorites`).
fn set_favorite(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["ids", "favorite"])?;
    let inv = cx.inv;
    let ids = ids_arg(args, inv)?;
    let favorite = args["favorite"]
        .as_bool()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`favorite` is required: true or false"))?;
    let mut originals = Vec::with_capacity(ids.len());
    for id in &ids {
        let original = actions::original(inv, *id).ok_or_else(|| err(ErrorCode::NotFound, "a link is broken"))?;
        if actions::in_type(inv, *id, FT_TRASH) {
            return Err(not_allowed("entries in the Trash are not marked as favorites: restore them first"));
        }
        if original == inv.root || actions::library(inv, original) || original == inv.lib_root {
            return Err(not_allowed(
                "My Inventory itself and the Library's entries cannot be marked as favorites",
            ));
        }
        originals.push(original);
    }
    marketplace_guard(inv, ids.iter().chain(&originals).copied())?;
    let change = actions::set_favorites(inv, &ids, favorite).map_err(not_allowed)?;
    let reply = json!({"ids": ids, "favorite": favorite});
    if change.operations.is_empty() {
        return Ok(nothing(reply));
    }
    let summary = if favorite {
        aurora_i18n::tr!("fs-mcp-ask-favorite-add", what = what(inv, &ids))
    } else {
        aurora_i18n::tr!("fs-mcp-ask-favorite-remove", what = what(inv, &ids))
    };
    Ok(inventory(change, summary, reply))
}

/// Rename for each entry (`finish_rename` of the window), checked one by
/// one with `actions::renameable` and sent as one change: one refusal
/// refuses them all, before anything is asked or sent.
fn batch_rename(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["items"])?;
    let inv = cx.inv;
    let list = args["items"]
        .as_array()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, "`items` is required: a list of {id, name}"))?;
    if list.is_empty() || list.len() > CHANGE_MAX {
        return Err(err(
            ErrorCode::InvalidArguments,
            format!("`items` must hold 1 to {CHANGE_MAX} entries"),
        ));
    }
    let mut seen = HashSet::new();
    let mut change = Mutation::default();
    let mut renamed = Vec::new();
    let mut unchanged = Vec::new();
    let mut first: Option<(String, String)> = None;
    for entry in list {
        if entry.as_object().is_none_or(|e| e.keys().any(|k| k != "id" && k != "name")) {
            return Err(err(ErrorCode::InvalidArguments, "each entry of `items` is {id, name}"));
        }
        let id = uuid_arg(entry, "id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "each entry needs an `id`"))?;
        if !seen.insert(id) {
            return Err(err(ErrorCode::InvalidArguments, format!("{id} is named twice")));
        }
        if !inv.items.contains_key(&id) && !inv.folders.contains_key(&id) {
            return Err(err(ErrorCode::NotFound, format!("no inventory entry {id}")));
        }
        marketplace_guard(inv, [id])?;
        let old = actions::name(inv, id);
        if !actions::renameable(inv, id) {
            return Err(not_allowed(format!(
                "“{old}” cannot be renamed (system folders, links, calling cards, Library entries, the Current Outfit, Received Items and items without Modify permission keep their names): nothing was renamed"
            )));
        }
        let name = entry["name"]
            .as_str()
            .ok_or_else(|| err(ErrorCode::InvalidArguments, "each entry needs a `name`"))?;
        let name = actions::clean_name(name).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
        if name == old {
            unchanged.push(id);
            continue;
        }
        let step = actions::patch(inv, id, aurora_llsd::llsd_map! { "name" => name.clone() })
            .map_err(|e| err(ErrorCode::InvalidArguments, format!("“{old}”: {e}")))?;
        change.operations.extend(step.operations);
        change.refresh.extend(step.refresh);
        first.get_or_insert_with(|| (old, name.clone()));
        renamed.push(json!({"id": id, "name": name}));
    }
    change.refresh.sort();
    change.refresh.dedup();
    let reply = json!({"renamed": renamed, "unchanged": unchanged});
    let Some((old, new)) = first else {
        return Ok(nothing(reply));
    };
    let summary = if renamed.len() == 1 {
        aurora_i18n::tr!("fs-mcp-ask-rename", old = old, new = new)
    } else {
        aurora_i18n::tr!("fs-mcp-ask-batch-rename", count = renamed.len(), old = old, new = new)
    };
    Ok(inventory(change, summary, reply))
}

/// The Image window of an item or folder: offered for an entry
/// `actions::mutable` allows, never for a link (`context_catalog`'s
/// "image" entries); an item held without Modify is refused too. "Choose image" takes a texture `thumbnail::usable`
/// allows (the picker's rule: Copy and Transfer, or the Library) or an id
/// `thumbnail::image_source_allowed` accepts; "Remove image" clears it.
fn set_thumbnail(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["id", "texture_id", "clear"])?;
    let inv = cx.inv;
    let id = uuid_arg(args, "id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "`id` is required"))?;
    let item = inv.items.get(&id);
    let folder = inv.folders.get(&id);
    if item.is_none() && folder.is_none() {
        return Err(err(ErrorCode::NotFound, "no such item or folder"));
    }
    marketplace_guard(inv, [id])?;
    // stricter than the window: an item's picture needs Modify, as its
    // name and description do
    if !actions::mutable(inv, id)
        || item.is_some_and(|it| matches!(it.asset_type, 24 | 25) || actions::held(inv, it) & actions::MODIFY == 0)
    {
        return Err(not_allowed(
            "this entry's picture cannot be changed: links, Library entries, the Current Outfit, Received Items and items without Modify permission keep theirs",
        ));
    }
    let clear = match &args["clear"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => return Err(err(ErrorCode::InvalidArguments, "`clear` must be true or false")),
    };
    let texture = uuid_arg(args, "texture_id")?;
    let current = item.map_or_else(|| folder.map(|f| f.info.thumbnail).unwrap_or_default(), |it| it.thumbnail);
    let name = actions::name(inv, id);
    match (texture, clear) {
        (Some(_), true) | (None, false) => Err(err(
            ErrorCode::InvalidArguments,
            "give `texture_id`, or `clear: true` to remove the picture",
        )),
        (None, true) => {
            let reply = json!({"id": id, "cleared": true});
            if current.is_nil() {
                return Ok(nothing(reply));
            }
            let change = actions::patch(inv, id, metadata_thumbnail(Uuid::nil())).map_err(not_allowed)?;
            let summary = aurora_i18n::tr!("fs-mcp-ask-picture-clear", name = name);
            Ok(inventory(change, summary, reply))
        }
        (Some(texture), false) => {
            let (texture, label) = match inv.items.get(&texture) {
                Some(_) => {
                    let original = actions::original(inv, texture)
                        .and_then(|o| inv.items.get(&o))
                        .ok_or_else(|| err(ErrorCode::NotFound, "the texture's link is broken"))?;
                    if !crate::ui::inventory::thumbnail::usable(inv, original) {
                        return Err(not_allowed(
                            "this texture cannot be a picture: it must be a texture that allows copying and transfer (or a Library texture), outside the Trash",
                        ));
                    }
                    (original.id, original.name.clone())
                }
                None => {
                    let demo = cx.session == super::tools::Session::Demo;
                    if !crate::ui::inventory::thumbnail::image_source_allowed(inv, &texture, false, demo) {
                        return Err(not_allowed(
                            "this texture is not one of your inventory's textures that allow copying and transfer",
                        ));
                    }
                    (texture, aurora_i18n::tr!("fs-mcp-picture-texture").to_owned())
                }
            };
            let summary = aurora_i18n::tr!("fs-mcp-ask-picture-set", name = name, texture = label);
            Ok(Planned {
                work: Work::Image { target: id, texture },
                summary,
                reply: json!({"id": id}),
                created: None,
            })
        }
    }
}
