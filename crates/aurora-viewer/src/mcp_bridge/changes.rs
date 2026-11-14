//! Folderstorm: the local assistant's changes (folderstorm/plans/mcp.md,
//! "Phase 2"), planned on the frame thread from the inventory model. Each
//! tool builds the change the inventory window or Appearance would build
//! for the same click, with the same rules: `world::inventory::actions`
//! for the inventory (Delete, Move to, Copy / Paste, Paste as link, Restore
//! item, New folder, Rename, the description field) and
//! `world::appearance::plan` for the outfits. Nothing here sends anything:
//! the app runs the planned work through its usual paths
//! (`app/local_assistant.rs`).
//!
//! A refusal by these rules is `not_allowed`, with the viewer's own message.
//! Stricter than the windows: the Marketplace Listings folder is left alone
//! (its listing dialogs have no assistant counterpart), and wearing a Library
//! item needs a copy first.

use super::tools::{Ctx, err, path, ready, scope, uuid_arg};
use crate::world::appearance::{self, Action};
use crate::world::inventory::{Inventory, actions};
use aurora_llsd::llsd_map;
use aurora_net::inventory::operations::{Mutation, Operation};
use folderstorm_mcp_bridge::protocol::CHANGE_MAX;
use folderstorm_mcp_bridge::{ErrorCode, ToolError};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

/// LLFolderType::FT_TRASH.
const FT_TRASH: i32 = 14;
/// The description field's limit (LLSidepanelItemInfo, 127 characters).
const DESCRIPTION_MAX: usize = 127;

/// What the app runs for a planned change.
#[derive(Debug, Clone)]
pub enum Work {
    /// Nothing to change (the name or description is already that one).
    Nothing,
    /// An inventory change, sent as the window sends it.
    Inventory(Mutation),
    /// An outfit change, through `App::apply_appearance_action`.
    Outfit(Action),
    /// A camera move (phase 3, `camera`).
    Camera(super::camera::Move),
    /// A picture, drawn by a photo job (phase 3, `picture`).
    Snapshot(super::picture::Request),
    /// The gallery's Take snapshot as this entry's picture (phase 3).
    Thumbnail(Uuid),
    /// Folderstorm: a new item, through the New menu's path (phase 3).
    Create(super::inventory_edits::Creation),
    /// Folderstorm: an entry's picture from a texture, through the Image
    /// window's path (phase 3).
    Image { target: Uuid, texture: Uuid },
    /// Folderstorm: a move, a take-off or landing, a teleport (phase 3,
    /// part 4).
    Navigate(super::navigation::Work),
    /// Folderstorm: a notecard's text or a script's source saved, through
    /// the editors' uploads (content editing).
    Content(super::content::Save),
    /// Folderstorm: an object's contents changed or the object taken, as
    /// the Contents tab and Take send it (mcp-objects.md).
    Object(Box<super::objects::Send>),
}

#[derive(Debug, Clone)]
pub struct Planned {
    pub work: Work,
    /// What the Ask dialog says, in the user's language.
    pub summary: String,
    /// The tool's answer once the change is confirmed.
    pub reply: Value,
    /// A folder this change creates, under its temporary id: the answer
    /// gives the server's id instead.
    pub created: Option<Uuid>,
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

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args[key]
        .as_str()
        .ok_or_else(|| err(ErrorCode::InvalidArguments, format!("`{key}` is required and must be a string")))
}

fn ids_arg(args: &Value, key: &str, inv: &Inventory) -> Result<Vec<Uuid>, ToolError> {
    let list = args[key].as_array().ok_or_else(|| {
        err(
            ErrorCode::InvalidArguments,
            format!("`{key}` is required and must be a list of UUIDs"),
        )
    })?;
    if list.is_empty() || list.len() > CHANGE_MAX {
        return Err(err(ErrorCode::InvalidArguments, format!("`{key}` must hold 1 to {CHANGE_MAX} ids")));
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(list.len());
    for value in list {
        let id = value
            .as_str()
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .ok_or_else(|| err(ErrorCode::InvalidArguments, format!("`{key}` holds something that is not a UUID")))?;
        if !inv.items.contains_key(&id) && !inv.folders.contains_key(&id) {
            return Err(err(ErrorCode::NotFound, format!("no inventory entry {id}")));
        }
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The destination of a move, copy or link: `folder_id` or `path`, required.
fn destination_arg(args: &Value, inv: &Inventory) -> Result<Uuid, ToolError> {
    scope(args, inv)?.ok_or_else(|| err(ErrorCode::InvalidArguments, "give the destination as `folder_id` or `path`"))
}

/// An outfit folder by `outfit_id` or `path`.
fn outfit_arg(args: &Value, inv: &Inventory) -> Result<Uuid, ToolError> {
    let id = uuid_arg(args, "outfit_id")?;
    let path = &args["path"];
    match (id, path.is_null()) {
        (Some(_), false) => Err(err(ErrorCode::InvalidArguments, "give `outfit_id` or `path`, not both")),
        (Some(id), true) if inv.folders.contains_key(&id) => Ok(id),
        (Some(_), true) => Err(err(ErrorCode::NotFound, "no such folder")),
        (None, false) => scope(&json!({"path": path}), inv)?.ok_or_else(|| err(ErrorCode::NotFound, "no such folder")),
        (None, true) => Err(err(ErrorCode::InvalidArguments, "give the outfit as `outfit_id` or `path`")),
    }
}

/// The Marketplace Listings folder and what it holds stay out of reach.
fn marketplace_guard(inv: &Inventory, ids: impl IntoIterator<Item = Uuid>) -> Result<(), ToolError> {
    if let Some(root) = crate::world::marketplace::root(inv)
        && ids.into_iter().any(|id| actions::under(inv, id, root))
    {
        return Err(not_allowed("the Marketplace Listings folder is not changed by an assistant"));
    }
    Ok(())
}

/// A folder as the dialog names it: its path, or its name for My
/// Inventory itself.
fn folder_label(inv: &Inventory, folder: Uuid) -> String {
    let path = path(inv, folder);
    if path.is_empty() { actions::name(inv, folder) } else { path }
}

/// The entries of a change as the dialog names them.
fn what(inv: &Inventory, ids: &[Uuid]) -> String {
    let first = ids.first().map(|id| actions::name(inv, *id)).unwrap_or_default();
    if ids.len() == 1 {
        aurora_i18n::tr!("fs-mcp-ask-one", name = first)
    } else {
        aurora_i18n::tr!("fs-mcp-ask-many", count = ids.len(), name = first)
    }
}

/// Plan one change, checking it as the window would.
pub fn plan(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    if super::camera::handles(tool) {
        return super::camera::plan(tool, args, cx);
    }
    // Folderstorm: phase 3, part 4 (navigation)
    if folderstorm_mcp_bridge::protocol::navigation::handles(tool) {
        return super::navigation::plan(tool, args, cx);
    }
    ready(cx)?;
    match tool {
        "inventory_create_folder" => create_folder(args, cx),
        "inventory_rename" => rename(args, cx),
        "inventory_describe" => describe(args, cx),
        "inventory_move" | "inventory_copy" | "inventory_link" => to_folder(tool, args, cx),
        "inventory_trash" => trash(args, cx),
        "inventory_restore" => restore(args, cx),
        "outfit_wear" | "outfit_take_off" => outfit(tool, args, cx),
        "items_wear" | "items_take_off" => items(tool, args, cx),
        // Folderstorm: a plan's execute, planned again (mcp-receipts.md, section 6)
        "inventory_execute_plan" => super::plans::plan(args, cx),
        // Folderstorm: a frozen content save, planned again (mcp-content.md)
        tool if super::content::is_save(tool) => super::content::plan(tool, args, cx),
        // Folderstorm: phase 3, part 2 (inventory gaps)
        _ => super::inventory_edits::plan(tool, args, cx).unwrap_or_else(|| Err(err(ErrorCode::UnknownTool, "unknown tool"))),
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

/// New folder (LLFolderBridge "New Folder"): where the window offers it.
fn create_folder(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["name", "folder_id", "path"])?;
    let inv = cx.inv;
    let parent = scope(args, inv)?.unwrap_or(inv.root);
    marketplace_guard(inv, [parent])?;
    if !actions::destination(inv, parent) {
        return Err(not_allowed(
            "this folder takes no new folder (the Library, Current Outfit, Received Items, Trash and My Outfits do not)",
        ));
    }
    let name = actions::clean_name(text_arg(args, "name")?).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
    let change = actions::create_folder(parent, &name).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
    let (id, name) = match change.operations.first() {
        Some(Operation::Folder { id, name, .. }) => (*id, name.clone()),
        _ => return Err(err(ErrorCode::Internal, "no folder planned")),
    };
    let summary = aurora_i18n::tr!("fs-mcp-ask-create-folder", name = name.clone(), folder = folder_label(inv, parent));
    let reply = json!({"folder_id": id, "name": name, "parent_id": parent});
    Ok(Planned {
        created: Some(id),
        ..inventory(change, summary, reply)
    })
}

/// Rename (`finish_rename` of the window): `actions::renameable`.
fn rename(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["id", "name"])?;
    let inv = cx.inv;
    let id = uuid_arg(args, "id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "`id` is required"))?;
    if !inv.items.contains_key(&id) && !inv.folders.contains_key(&id) {
        return Err(err(ErrorCode::NotFound, "no such item or folder"));
    }
    marketplace_guard(inv, [id])?;
    if !actions::renameable(inv, id) {
        return Err(not_allowed(
            "this entry cannot be renamed: system folders, links, calling cards, Library entries, the Current Outfit, Received Items and items without Modify permission keep their names",
        ));
    }
    let name = actions::clean_name(text_arg(args, "name")?).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
    let old = actions::name(inv, id);
    let reply = json!({"id": id, "name": name});
    if name == old {
        return Ok(Planned {
            work: Work::Nothing,
            summary: String::new(),
            reply,
            created: None,
        });
    }
    let change = actions::patch(inv, id, llsd_map! { "name" => name.clone() }).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
    let summary = aurora_i18n::tr!("fs-mcp-ask-rename", old = old, new = name);
    Ok(inventory(change, summary, reply))
}

/// The description field of an item's preview or properties: editable on an
/// item of the agent's own inventory held with Modify (`actions::mutable`
/// and `actions::held`, as `ui/inventory/dialogs.rs` reads them). Links keep
/// their original's text: refused, stricter than the properties panel.
fn describe(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["item_id", "description"])?;
    let inv = cx.inv;
    let id = uuid_arg(args, "item_id")?.ok_or_else(|| err(ErrorCode::InvalidArguments, "`item_id` is required"))?;
    if inv.folders.contains_key(&id) {
        return Err(err(ErrorCode::InvalidArguments, "folders have no description"));
    }
    let item = inv.items.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "no such item"))?;
    marketplace_guard(inv, [id])?;
    if matches!(item.asset_type, 24 | 25) || !actions::mutable(inv, id) || actions::held(inv, item) & actions::MODIFY == 0 {
        return Err(not_allowed(
            "this item's description cannot be changed: links, Library items, the Current Outfit, Received Items and items without Modify permission keep theirs",
        ));
    }
    let text = text_arg(args, "description")?;
    if text.chars().count() > DESCRIPTION_MAX {
        return Err(err(
            ErrorCode::InvalidArguments,
            format!("a description holds {DESCRIPTION_MAX} characters at most"),
        ));
    }
    let reply = json!({"id": id, "description": text});
    if text == item.desc {
        return Ok(Planned {
            work: Work::Nothing,
            summary: String::new(),
            reply,
            created: None,
        });
    }
    let change = actions::patch(inv, id, llsd_map! { "desc" => text.to_owned() }).map_err(|e| err(ErrorCode::InvalidArguments, e))?;
    let summary = aurora_i18n::tr!("fs-mcp-ask-describe", name = item.name.clone(), text = text.to_owned());
    Ok(inventory(change, summary, reply))
}

/// Move to (`actions::move_selection`), Copy then Paste and Paste as link
/// (`actions::paste`).
fn to_folder(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["ids", "folder_id", "path"])?;
    let inv = cx.inv;
    let ids = ids_arg(args, "ids", inv)?;
    let to = destination_arg(args, inv)?;
    marketplace_guard(inv, ids.iter().copied().chain([to]))?;
    let folder = folder_label(inv, to);
    let what = what(inv, &ids);
    let (change, summary, key) = match tool {
        "inventory_move" => {
            if Some(to) == actions::system(inv, FT_TRASH) || actions::in_type(inv, to, FT_TRASH) {
                return Err(err(
                    ErrorCode::InvalidArguments,
                    "to put something in the Trash, use inventory_trash",
                ));
            }
            let change = actions::move_selection(inv, &ids, to, &cx.protected, &cx.worn).map_err(not_allowed)?;
            (change, aurora_i18n::tr!("fs-mcp-ask-move", what = what, folder = folder), "moved")
        }
        "inventory_copy" => {
            let change = actions::paste(inv, &ids, to, false, false, &cx.protected, &cx.worn).map_err(not_allowed)?;
            (change, aurora_i18n::tr!("fs-mcp-ask-copy", what = what, folder = folder), "copied")
        }
        _ => {
            let change = actions::paste(inv, &ids, to, false, true, &cx.protected, &cx.worn).map_err(not_allowed)?;
            (change, aurora_i18n::tr!("fs-mcp-ask-link", what = what, folder = folder), "linked")
        }
    };
    let mut reply = json!({"folder_id": to});
    reply[key] = json!(ids);
    Ok(inventory(change, summary, reply))
}

/// Delete in the window: the selection moves to the Trash, with the
/// Delete rule (`actions::move_selection` into the Trash checks
/// `actions::removable` and the worn items, as `ui/inventory/delete.rs`
/// does). Entries already in the Trash would be purged by the window: never
/// here.
fn trash(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["ids"])?;
    let inv = cx.inv;
    let ids = ids_arg(args, "ids", inv)?;
    let bin = actions::system(inv, FT_TRASH).ok_or_else(|| err(ErrorCode::NotReady, "the Trash folder is not loaded"))?;
    marketplace_guard(inv, ids.iter().copied())?;
    if ids.iter().any(|id| actions::under(inv, *id, bin)) {
        return Err(not_allowed(
            "that is already in the Trash; deleting it for good is never allowed to an assistant",
        ));
    }
    let change = actions::move_selection(inv, &ids, bin, &cx.protected, &cx.worn).map_err(not_allowed)?;
    let summary = aurora_i18n::tr!("fs-mcp-ask-trash", what = what(inv, &ids));
    Ok(inventory(change, summary, json!({"trashed": ids})))
}

/// "Restore item" of the Trash's menu (`Command::DefaultFolder` of
/// `ui/inventory/context_catalog.rs`): each entry to the folder of its
/// type, through `actions::move_selection`.
fn restore(args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    known(args, &["ids"])?;
    let inv = cx.inv;
    let ids = ids_arg(args, "ids", inv)?;
    let bin = actions::system(inv, FT_TRASH).ok_or_else(|| err(ErrorCode::NotReady, "the Trash folder is not loaded"))?;
    let mut change = Mutation::default();
    let mut restored = Vec::new();
    for id in actions::roots(inv, &ids) {
        if id == bin || !actions::under(inv, id, bin) {
            return Err(err(ErrorCode::InvalidArguments, format!("{id} is not in the Trash")));
        }
        let to = actions::default_folder(inv, id).ok_or_else(|| err(ErrorCode::NotFound, "no folder to restore it to"))?;
        let step = actions::move_selection(inv, &[id], to, &cx.protected, &cx.worn).map_err(not_allowed)?;
        change.operations.extend(step.operations);
        change.refresh.extend(step.refresh);
        restored.push(json!({"id": id, "folder_id": to}));
    }
    let summary = aurora_i18n::tr!("fs-mcp-ask-restore", what = what(inv, &ids));
    Ok(inventory(change, summary, json!({"restored": restored})))
}

/// Check an outfit action with Appearance's own planner, as the click
/// would, before it is asked for or run.
fn outfit_work(cx: &Ctx, action: Action, summary: String, reply: Value) -> Result<Planned, ToolError> {
    appearance::plan(cx.inv, action.clone(), &cx.outfit_points).map_err(not_allowed)?;
    Ok(Planned {
        work: Work::Outfit(action),
        summary,
        reply,
        created: None,
    })
}

/// Wear (replace or add) and Take off of an outfit folder in Appearance
/// (`Action::Wear`, `Action::RemoveOutfit`).
fn outfit(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    let inv = cx.inv;
    let wear = tool == "outfit_wear";
    known(
        args,
        if wear {
            &["outfit_id", "path", "mode"]
        } else {
            &["outfit_id", "path"]
        },
    )?;
    let folder = outfit_arg(args, inv)?;
    if appearance::cof(inv) == Some(folder) || folder == inv.root || folder == inv.lib_root {
        return Err(err(ErrorCode::InvalidArguments, "that folder is not an outfit"));
    }
    if actions::library(inv, folder) || actions::in_type(inv, folder, FT_TRASH) {
        return Err(not_allowed(
            "outfits in the Library or the Trash are not worn from there: copy or restore them first",
        ));
    }
    let name = actions::name(inv, folder);
    let reply = json!({"outfit_id": folder, "done": true, "next": "call wait_for_bake to know when the avatar has changed"});
    if !wear {
        let summary = aurora_i18n::tr!("fs-mcp-ask-outfit-off", name = name);
        return outfit_work(cx, Action::RemoveOutfit(folder), summary, reply);
    }
    let append = match &args["mode"] {
        Value::Null => false,
        Value::String(m) if m == "replace" => false,
        Value::String(m) if m == "add" => true,
        _ => return Err(err(ErrorCode::InvalidArguments, "`mode` is `replace` or `add`")),
    };
    let summary = if append {
        aurora_i18n::tr!("fs-mcp-ask-outfit-add", name = name)
    } else {
        aurora_i18n::tr!("fs-mcp-ask-outfit-replace", name = name)
    };
    outfit_work(cx, Action::Wear(folder, append), summary, reply)
}

/// Wear / Add and Take off of items (`Action::WearItems`,
/// `Action::RemoveItems`), on the originals of links.
fn items(tool: &str, args: &Value, cx: &Ctx) -> Result<Planned, ToolError> {
    let inv = cx.inv;
    let wear = tool == "items_wear";
    known(args, if wear { &["item_ids", "mode"] } else { &["item_ids"] })?;
    let given = ids_arg(args, "item_ids", inv)?;
    let mut ids = Vec::with_capacity(given.len());
    for id in given {
        let original = actions::original(inv, id).ok_or_else(|| err(ErrorCode::NotFound, "a link is broken"))?;
        let item = inv
            .items
            .get(&original)
            .ok_or_else(|| err(ErrorCode::InvalidArguments, "folders are worn with outfit_wear"))?;
        if !matches!(item.asset_type, 5 | 6 | 13) {
            return Err(err(
                ErrorCode::InvalidArguments,
                format!("“{}” is not clothing, a body part or an object", item.name),
            ));
        }
        if !ids.contains(&original) {
            ids.push(original);
        }
    }
    let reply = json!({"item_ids": ids, "done": true, "next": "call wait_for_bake to know when the avatar has changed"});
    if wear {
        if let Some(id) = ids
            .iter()
            .find(|id| actions::library(inv, **id) || actions::in_type(inv, **id, FT_TRASH))
        {
            return Err(not_allowed(format!(
                "“{}” is in the Library or the Trash: copy it to your inventory (inventory_copy) or restore it first",
                actions::name(inv, *id)
            )));
        }
        let replace = match &args["mode"] {
            Value::Null => false,
            Value::String(m) if m == "add" => false,
            Value::String(m) if m == "replace" => true,
            _ => return Err(err(ErrorCode::InvalidArguments, "`mode` is `add` or `replace`")),
        };
        let summary = aurora_i18n::tr!("fs-mcp-ask-wear", what = what(inv, &ids));
        return outfit_work(
            cx,
            Action::WearItems {
                items: ids,
                replace,
                point: 0,
            },
            summary,
            reply,
        );
    }
    for id in &ids {
        if !cx.worn.contains(id) {
            return Err(err(
                ErrorCode::InvalidArguments,
                format!("“{}” is not worn", actions::name(inv, *id)),
            ));
        }
        if inv.items.get(id).is_some_and(|it| it.asset_type == 13) {
            return Err(not_allowed("body parts cannot be taken off, only replaced by another one"));
        }
    }
    let summary = aurora_i18n::tr!("fs-mcp-ask-take-off", what = what(inv, &ids));
    outfit_work(cx, Action::RemoveItems(ids), summary, reply)
}
